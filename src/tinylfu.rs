//! TinyLFU cache — sharded arena-backed LRU main + per-shard Count-Min Sketch.

use core::borrow::Borrow;
use core::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::num::NonZeroUsize;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::sharding::{self, Sharded};
use crate::util::MutexExt;

const SKETCH_DEPTH: usize = 4;
const MIN_SKETCH_WIDTH: usize = 64;
/// Upper bound on the sketch row width (4 MiB of counters per shard at
/// depth 4). Keeps the width computation from overflowing for huge
/// capacities; past this size the frequency estimate is a little coarser.
const MAX_SKETCH_WIDTH: usize = 1 << 20;

/// A bounded, thread-safe cache with **admission control**.
///
/// `TinyLfuCache` tracks the access frequency of *every* key it observes —
/// including keys that aren't (yet) in the cache — using a Count-Min Sketch.
/// On capacity overflow, an incoming key is **admitted only if its
/// estimated frequency exceeds the LRU victim's**. One-hit-wonders are
/// rejected at the door instead of evicting hot entries.
///
/// A successful [`insert`](Cache::insert) call **does not guarantee** the
/// value is in the cache. The admission filter may reject it. Callers that
/// need strict insertion guarantees should use `LruCache` or `LfuCache`.
///
/// # Implementation
///
/// Sharded into up to 16 independent arenas keyed by hash of `K`. **Each
/// shard owns its own Count-Min Sketch** — the frequency signal is
/// per-shard, not global. This is a deliberate trade-off: a global sketch
/// would force every access to lock a shared structure, defeating the
/// point of sharding. Per-shard sketches still capture the local frequency
/// signal accurately, which is what the local admission decision needs.
///
/// Eviction is approximate (per-shard LRU). Tiny caches (< 32 entries)
/// use a single shard and retain strict global semantics.
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, TinyLfuCache};
///
/// let cache: TinyLfuCache<&'static str, u32> =
///     TinyLfuCache::new(4).expect("capacity > 0");
///
/// for _ in 0..16 {
///     let _ = cache.get(&"hot");
///     let _ = cache.insert("hot", 1);
/// }
///
/// assert_eq!(cache.get(&"hot"), Some(1));
/// ```
pub struct TinyLfuCache<K, V> {
    capacity: NonZeroUsize,
    sharded: Sharded<Inner<K, V>>,
}

struct Node<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

struct Inner<K, V> {
    capacity: NonZeroUsize,
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
    fn with_capacity(capacity: NonZeroUsize) -> Self {
        let reserve = sharding::initial_reserve(capacity);
        Self {
            capacity,
            nodes: Vec::with_capacity(reserve),
            free: Vec::new(),
            head: None,
            tail: None,
            map: HashMap::with_capacity(reserve),
            sketch: CountMinSketch::new(capacity.get()),
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
        let num_shards = sharding::shard_count(capacity);
        let per_shard = sharding::per_shard_capacity(capacity, num_shards);
        let sharded = Sharded::from_factory(num_shards, |_| Inner::with_capacity(per_shard));
        Self { capacity, sharded }
    }

    /// Like [`get`](Cache::get), but takes any borrowed form of the key,
    /// so a `TinyLfuCache<String, V>` can be queried with a `&str` without
    /// building a `String`. Counts as an access for the frequency sketch.
    ///
    /// # Example
    ///
    /// ```
    /// use cache_mod::{Cache, TinyLfuCache};
    ///
    /// let cache: TinyLfuCache<String, u32> = TinyLfuCache::new(8).expect("capacity > 0");
    /// cache.insert("hits".to_string(), 3);
    /// assert_eq!(cache.get_by("hits"), Some(3));
    /// ```
    pub fn get_by<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        inner.sketch.increment(key);
        let idx = *inner.map.get(key)?;
        inner.promote(idx);
        inner.nodes[idx].as_ref().map(|n| n.value.clone())
    }

    /// Like [`contains_key`](Cache::contains_key), but takes any borrowed
    /// form of the key.
    pub fn contains_key_by<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.sharded
            .shard_for(key)
            .lock_recover()
            .map
            .contains_key(key)
    }

    /// Like [`remove`](Cache::remove), but takes any borrowed form of the
    /// key.
    pub fn remove_by<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let idx = inner.map.remove(key)?;
        inner.unlink(idx);
        let node = inner.dealloc(idx);
        Some(node.value)
    }
}

impl<K, V> Cache<K, V> for TinyLfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        self.get_by(key)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.sharded.shard_for(&key).lock_recover();
        inner.sketch.increment(&key);

        if let Some(&idx) = inner.map.get(&key) {
            let old = inner.nodes[idx]
                .as_mut()
                .map(|n| core::mem::replace(&mut n.value, value))
                .unwrap_or_else(|| unreachable!("mapped index must be occupied"));
            inner.promote(idx);
            return Some(old);
        }

        if inner.map.len() >= inner.capacity.get() {
            let candidate_freq = inner.sketch.estimate(&key);
            let tail_idx = inner.tail?;
            let shard = &mut *inner;
            let victim = shard.nodes[tail_idx]
                .as_ref()
                .unwrap_or_else(|| unreachable!("tail must be occupied"));
            if candidate_freq <= shard.sketch.estimate(&victim.key) {
                return None;
            }
            // Index entry first, then the slot: a panic in the user's
            // `Hash` or `Eq` leaves the index and the arena in step.
            let _ = shard.map.remove(&victim.key);
            inner.unlink(tail_idx);
            let _ = inner.dealloc(tail_idx);
        }

        let idx = inner.alloc(Node {
            key: key.clone(),
            value,
            prev: None,
            next: None,
        });
        let _ = inner.map.insert(key, idx);
        inner.push_front(idx);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        self.remove_by(key)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.contains_key_by(key)
    }

    fn len(&self) -> usize {
        self.sharded
            .iter()
            .map(|m| m.lock_recover().map.len())
            .sum()
    }

    fn clear(&self) {
        for mutex in self.sharded.iter() {
            let mut inner = mutex.lock_recover();
            inner.nodes.clear();
            inner.free.clear();
            inner.head = None;
            inner.tail = None;
            inner.map.clear();
            inner.sketch.reset();
        }
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

// -----------------------------------------------------------------------------
// Count-Min Sketch (per-shard)
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
        let width = capacity
            .saturating_mul(2)
            .clamp(MIN_SKETCH_WIDTH, MAX_SKETCH_WIDTH)
            .next_power_of_two();
        let sample_window = (capacity as u64).saturating_mul(10).max(64);
        Self {
            counters: vec![0; width.saturating_mul(SKETCH_DEPTH)],
            width,
            width_u64: width as u64,
            samples: 0,
            sample_window,
        }
    }

    fn estimate<K: Hash + ?Sized>(&self, key: &K) -> u8 {
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

    fn increment<K: Hash + ?Sized>(&mut self, key: &K) {
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

    fn cell<K: Hash + ?Sized>(&self, d: usize, key: &K) -> usize {
        let h = hash_with_seed(key, d as u64);
        let col = (h % self.width_u64) as usize;
        d.saturating_mul(self.width).saturating_add(col)
    }
}

fn hash_with_seed<K: Hash + ?Sized>(key: &K, seed: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    key.hash(&mut hasher);
    hasher.finish()
}
