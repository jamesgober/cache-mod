//! TinyLFU cache — Count-Min Sketch frequency estimator + admission filter
//! on top of an arena-backed LRU main cache.

use core::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::util::MutexExt;

/// Count-Min Sketch depth (number of independent hash rows).
const SKETCH_DEPTH: usize = 4;

/// Width floor — the sketch is never narrower than this, regardless of
/// configured capacity. Guards small-capacity caches from degenerate
/// collision rates.
const MIN_SKETCH_WIDTH: usize = 64;

/// A bounded, thread-safe cache with **admission control**.
///
/// `TinyLfuCache` tracks the access frequency of *every* key it observes —
/// including keys that aren't (yet) in the cache — using a fixed-size
/// [Count-Min Sketch][cms]. On capacity overflow, an incoming key is
/// **admitted only if its estimated frequency exceeds the LRU victim's**.
/// One-hit-wonders are rejected at the door instead of evicting hot entries.
///
/// This is a deliberate semantic deviation from `LruCache` / `LfuCache` /
/// `TtlCache`: a successful [`insert`](Cache::insert) call **does not
/// guarantee** that the value is in the cache. The admission filter may
/// have rejected it. Callers that need strict insertion guarantees should
/// use `LruCache` or `LfuCache` instead.
///
/// 0.6.0 implementation:
///
/// - main cache is arena-backed (O(1) promote, O(1) evict — same shape as `LruCache`)
/// - depth-4 Count-Min Sketch with `u8` saturating counters
/// - width = `max(MIN_SKETCH_WIDTH, 2 × capacity)`, rounded to the next power of two
/// - periodic frequency decay: every `10 × capacity` increments, every counter
///   is right-shifted by 1 (W-TinyLFU "aging" step, keeps the sketch responsive
///   to shifting workloads)
/// - main cache uses LRU ordering; eviction victim = least-recently-accessed
/// - lock-minimized via `&self` + `Mutex<Inner>`; a sharded / lock-free variant
///   lands in 0.7.0 without changing this public surface
///
/// [cms]: https://en.wikipedia.org/wiki/Count%E2%80%93min_sketch
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, TinyLfuCache};
///
/// let cache: TinyLfuCache<&'static str, u32> =
///     TinyLfuCache::new(4).expect("capacity > 0");
///
/// // Build up the frequency signal for "hot".
/// for _ in 0..16 {
///     let _ = cache.get(&"hot");
///     let _ = cache.insert("hot", 1);
/// }
///
/// // A subsequent insert will see "hot" as warm in the sketch.
/// assert_eq!(cache.get(&"hot"), Some(1));
/// ```
pub struct TinyLfuCache<K, V> {
    capacity: NonZeroUsize,
    inner: Mutex<Inner<K, V>>,
}

struct Node<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

struct Inner<K, V> {
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
    map: HashMap<K, usize>,
    sketch: CountMinSketch,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn with_capacity(cap: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(cap),
            free: Vec::new(),
            head: None,
            tail: None,
            map: HashMap::with_capacity(cap),
            sketch: CountMinSketch::new(cap),
        }
    }

    fn alloc(&mut self, node: Node<K, V>) -> usize {
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    fn dealloc(&mut self, idx: usize) -> Node<K, V> {
        let node = self.nodes[idx]
            .take()
            .unwrap_or_else(|| unreachable!("arena slot must be occupied"));
        self.free.push(idx);
        node
    }

    fn unlink(&mut self, idx: usize) {
        let (prev, next) = {
            let n = self.nodes[idx]
                .as_ref()
                .unwrap_or_else(|| unreachable!("unlink target must be occupied"));
            (n.prev, n.next)
        };
        match prev {
            Some(p) => {
                self.nodes[p]
                    .as_mut()
                    .unwrap_or_else(|| unreachable!())
                    .next = next
            }
            None => self.head = next,
        }
        match next {
            Some(n) => {
                self.nodes[n]
                    .as_mut()
                    .unwrap_or_else(|| unreachable!())
                    .prev = prev
            }
            None => self.tail = prev,
        }
        if let Some(n) = self.nodes[idx].as_mut() {
            n.prev = None;
            n.next = None;
        }
    }

    fn push_front(&mut self, idx: usize) {
        let old_head = self.head;
        if let Some(n) = self.nodes[idx].as_mut() {
            n.prev = None;
            n.next = old_head;
        }
        if let Some(h) = old_head {
            if let Some(n) = self.nodes[h].as_mut() {
                n.prev = Some(idx);
            }
        } else {
            self.tail = Some(idx);
        }
        self.head = Some(idx);
    }

    fn promote(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return;
        }
        self.unlink(idx);
        self.push_front(idx);
    }
}

impl<K, V> TinyLfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a cache with the given entry-count capacity.
    ///
    /// Returns [`CacheError::InvalidCapacity`] if `capacity == 0`.
    ///
    /// # Example
    ///
    /// ```
    /// use cache_mod::TinyLfuCache;
    ///
    /// let cache: TinyLfuCache<String, u32> =
    ///     TinyLfuCache::new(256).expect("capacity > 0");
    /// ```
    pub fn new(capacity: usize) -> Result<Self, CacheError> {
        let cap = NonZeroUsize::new(capacity).ok_or(CacheError::InvalidCapacity)?;
        Ok(Self::with_capacity(cap))
    }

    /// Creates a cache with the given non-zero capacity. Infallible.
    ///
    /// # Example
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use cache_mod::TinyLfuCache;
    ///
    /// let cap = NonZeroUsize::new(256).expect("256 != 0");
    /// let cache: TinyLfuCache<String, u32> = TinyLfuCache::with_capacity(cap);
    /// ```
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        let cap = capacity.get();
        Self {
            capacity,
            inner: Mutex::new(Inner::with_capacity(cap)),
        }
    }
}

impl<K, V> Cache<K, V> for TinyLfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        // Every observation feeds the sketch, even on cache miss.
        inner.sketch.increment(key);
        let idx = *inner.map.get(key)?;
        inner.promote(idx);
        inner.nodes[idx].as_ref().map(|n| n.value.clone())
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        inner.sketch.increment(&key);

        // Live update: existing key always succeeds (no admission check).
        if let Some(&idx) = inner.map.get(&key) {
            let old = inner.nodes[idx]
                .as_mut()
                .map(|n| core::mem::replace(&mut n.value, value))
                .unwrap_or_else(|| unreachable!("mapped index must be occupied"));
            inner.promote(idx);
            return Some(old);
        }

        // New key. Admission filter kicks in at capacity.
        if inner.map.len() >= self.capacity.get() {
            let candidate_freq = inner.sketch.estimate(&key);
            let tail_idx = inner.tail?;
            let victim_key = inner.nodes[tail_idx]
                .as_ref()
                .map(|n| n.key.clone())
                .unwrap_or_else(|| unreachable!("tail must be occupied"));
            let victim_freq = inner.sketch.estimate(&victim_key);
            if candidate_freq <= victim_freq {
                // Reject — value dropped at function exit.
                return None;
            }
            inner.unlink(tail_idx);
            let _ = inner.dealloc(tail_idx);
            let _ = inner.map.remove(&victim_key);
        }

        let idx = inner.alloc(Node {
            key: key.clone(),
            value,
            prev: None,
            next: None,
        });
        inner.push_front(idx);
        let _ = inner.map.insert(key, idx);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let idx = inner.map.remove(key)?;
        inner.unlink(idx);
        let node = inner.dealloc(idx);
        Some(node.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.inner.lock_recover().map.contains_key(key)
    }

    fn len(&self) -> usize {
        self.inner.lock_recover().map.len()
    }

    fn clear(&self) {
        let mut inner = self.inner.lock_recover();
        inner.nodes.clear();
        inner.free.clear();
        inner.head = None;
        inner.tail = None;
        inner.map.clear();
        inner.sketch.reset();
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

// -----------------------------------------------------------------------------
// Count-Min Sketch
// -----------------------------------------------------------------------------

struct CountMinSketch {
    counters: Vec<u8>,
    width: usize,
    width_u64: u64,
    samples: u64,
    sample_window: u64,
}

impl CountMinSketch {
    fn new(capacity: usize) -> Self {
        let mut width = capacity.saturating_mul(2).max(MIN_SKETCH_WIDTH);
        width = width.next_power_of_two();
        let sample_window = (capacity as u64).saturating_mul(10).max(64);
        Self {
            counters: vec![0; width.saturating_mul(SKETCH_DEPTH)],
            width,
            width_u64: width as u64,
            samples: 0,
            sample_window,
        }
    }

    fn estimate<K: Hash>(&self, key: &K) -> u8 {
        let mut min = u8::MAX;
        for d in 0..SKETCH_DEPTH {
            let idx = self.cell(d, key);
            let observed = *self.counters.get(idx).unwrap_or(&0);
            if observed < min {
                min = observed;
            }
        }
        min
    }

    fn increment<K: Hash>(&mut self, key: &K) {
        for d in 0..SKETCH_DEPTH {
            let idx = self.cell(d, key);
            if let Some(slot) = self.counters.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }
        self.samples = self.samples.saturating_add(1);
        if self.samples >= self.sample_window {
            self.age();
            self.samples = 0;
        }
    }

    fn reset(&mut self) {
        for c in self.counters.iter_mut() {
            *c = 0;
        }
        self.samples = 0;
    }

    fn age(&mut self) {
        for c in self.counters.iter_mut() {
            *c >>= 1;
        }
    }

    fn cell<K: Hash>(&self, d: usize, key: &K) -> usize {
        let h = hash_with_seed(key, d as u64);
        let col = (h % self.width_u64) as usize;
        d.saturating_mul(self.width).saturating_add(col)
    }
}

fn hash_with_seed<K: Hash>(key: &K, seed: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    key.hash(&mut hasher);
    hasher.finish()
}
