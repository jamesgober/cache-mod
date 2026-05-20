//! TinyLFU cache — Count-Min Sketch frequency estimator + admission filter
//! on top of an LRU-ordered main cache.

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
/// Reference implementation details:
///
/// - depth-4 Count-Min Sketch with `u8` saturating counters
/// - width = `max(MIN_SKETCH_WIDTH, 2 × capacity)`, rounded to the next power of two
/// - periodic frequency decay: every `10 × capacity` increments, every counter
///   is right-shifted by 1 (this is the "aging" step from the W-TinyLFU paper,
///   which keeps the sketch responsive to shifting workloads)
/// - main cache uses LRU ordering; eviction victim = least-recently-accessed
/// - lock-minimized via `&self` + `Mutex<Inner>`; a true sharded / lock-free
///   variant lands in a later minor without changing this public surface
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
///
/// [cms]: https://en.wikipedia.org/wiki/Count%E2%80%93min_sketch
pub struct TinyLfuCache<K, V> {
    capacity: NonZeroUsize,
    inner: Mutex<Inner<K, V>>,
}

struct Entry<V> {
    value: V,
    last_access: u64,
}

struct Inner<K, V> {
    map: HashMap<K, Entry<V>>,
    sketch: CountMinSketch,
    clock: u64,
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
            inner: Mutex::new(Inner {
                map: HashMap::with_capacity(cap),
                sketch: CountMinSketch::new(cap),
                clock: 0,
            }),
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
        // Every observation feeds the sketch, even on cache miss — that's
        // how the cache learns which keys are "hot" before they are admitted.
        inner.sketch.increment(key);
        inner.clock = inner.clock.wrapping_add(1);
        let now = inner.clock;
        let entry = inner.map.get_mut(key)?;
        entry.last_access = now;
        Some(entry.value.clone())
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        inner.sketch.increment(&key);
        inner.clock = inner.clock.wrapping_add(1);
        let now = inner.clock;

        // Live update: existing key always succeeds (no admission check).
        if let Some(existing) = inner.map.get_mut(&key) {
            let old = core::mem::replace(&mut existing.value, value);
            existing.last_access = now;
            return Some(old);
        }

        // New key. If at capacity, run the admission filter.
        if inner.map.len() >= self.capacity.get() {
            let candidate_freq = inner.sketch.estimate(&key);
            let victim = find_lru_victim(&inner.map);
            if let Some(victim_key) = victim {
                let victim_freq = inner.sketch.estimate(&victim_key);
                if candidate_freq <= victim_freq {
                    // Reject the admission. The value is dropped at the
                    // end of the function. Returning `None` matches the
                    // "no prior entry was present" semantic.
                    return None;
                }
                let _ = inner.map.remove(&victim_key);
            }
        }

        let _ = inner.map.insert(
            key,
            Entry {
                value,
                last_access: now,
            },
        );
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        inner.map.remove(key).map(|e| e.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.inner.lock_recover().map.contains_key(key)
    }

    fn len(&self) -> usize {
        self.inner.lock_recover().map.len()
    }

    fn clear(&self) {
        let mut inner = self.inner.lock_recover();
        inner.map.clear();
        inner.sketch.reset();
        inner.clock = 0;
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

fn find_lru_victim<K, V>(map: &HashMap<K, Entry<V>>) -> Option<K>
where
    K: Clone,
{
    map.iter()
        .min_by_key(|(_, e)| e.last_access)
        .map(|(k, _)| k.clone())
}

// -----------------------------------------------------------------------------
// Count-Min Sketch
// -----------------------------------------------------------------------------

/// A small Count-Min Sketch with `u8` saturating counters and periodic
/// halving. Used as the frequency estimator behind `TinyLfuCache`'s
/// admission filter.
struct CountMinSketch {
    counters: Vec<u8>,
    width: usize,
    /// `width` as `u64` — pre-cached because every probe modulos by it.
    width_u64: u64,
    samples: u64,
    /// Number of increments between sketch halvings (the "aging" trigger).
    sample_window: u64,
}

impl CountMinSketch {
    fn new(capacity: usize) -> Self {
        let mut width = capacity.saturating_mul(2).max(MIN_SKETCH_WIDTH);
        // Round up to the next power of two so the modulus could later be
        // swapped for a mask without changing semantics.
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

    /// Halve every counter — the W-TinyLFU "aging" step. Recent activity
    /// dominates over time-bygone activity, which lets the cache follow
    /// workload shifts instead of being locked in to the first hot set.
    fn age(&mut self) {
        for c in self.counters.iter_mut() {
            *c >>= 1;
        }
    }

    /// Compute the absolute counter index for the `d`-th sketch row.
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
