//! Least-Frequently-Used (LFU) cache — sharded, per-shard BTreeMap priority index.

use core::hash::Hash;
use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::sharding::{self, Sharded};
use crate::util::MutexExt;

/// A bounded, thread-safe LFU cache.
///
/// Each entry carries a counter that is incremented on every [`get`](Cache::get)
/// or [`insert`](Cache::insert) of an already-present key. On overflow, the
/// entry with the **lowest counter** is evicted; ties are broken in favour of
/// evicting the **least-recently-accessed** entry.
///
/// [`contains_key`](Cache::contains_key) is a query and does not increment
/// the counter or touch access order.
///
/// # Implementation
///
/// Sharded into up to 16 independent stores keyed by hash of `K`. Each
/// shard pairs a `HashMap<K, Entry<V>>` for value lookup with a
/// `BTreeMap<(count, age), K>` ordered priority index for O(log n) eviction.
///
/// Eviction is **per-shard approximate** LFU — the entry evicted on
/// overflow is the lowest-counter entry in the affected shard, not
/// necessarily the lowest-counter entry globally. Tiny caches (< 32 entries)
/// use a single shard and retain strict global semantics.
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, LfuCache};
///
/// let cache: LfuCache<&'static str, u32> = LfuCache::new(2).expect("capacity > 0");
///
/// cache.insert("a", 1);
/// cache.insert("b", 2);
///
/// assert_eq!(cache.get(&"a"), Some(1));
/// assert_eq!(cache.get(&"a"), Some(1));
///
/// cache.insert("c", 3);
/// assert_eq!(cache.get(&"b"), None);
/// assert_eq!(cache.get(&"a"), Some(1));
/// assert_eq!(cache.get(&"c"), Some(3));
/// ```
pub struct LfuCache<K, V> {
    capacity: NonZeroUsize,
    sharded: Sharded<Inner<K, V>>,
}

struct Entry<V> {
    value: V,
    count: u64,
    age: u64,
}

struct Inner<K, V> {
    capacity: NonZeroUsize,
    map: HashMap<K, Entry<V>>,
    by_priority: BTreeMap<(u64, u64), K>,
    clock: u64,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn with_capacity(capacity: NonZeroUsize) -> Self {
        let cap = capacity.get();
        Self {
            capacity,
            map: HashMap::with_capacity(cap),
            by_priority: BTreeMap::new(),
            clock: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }
}

impl<K, V> LfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a cache with the given capacity.
    ///
    /// Returns [`CacheError::InvalidCapacity`] if `capacity == 0`.
    ///
    /// # Example
    ///
    /// ```
    /// use cache_mod::LfuCache;
    ///
    /// let cache: LfuCache<String, u32> = LfuCache::new(128).expect("capacity > 0");
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
    /// use cache_mod::LfuCache;
    ///
    /// let cap = NonZeroUsize::new(64).expect("64 != 0");
    /// let cache: LfuCache<String, u32> = LfuCache::with_capacity(cap);
    /// ```
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        let num_shards = sharding::shard_count(capacity);
        let per_shard = sharding::per_shard_capacity(capacity, num_shards);
        let sharded = Sharded::from_factory(num_shards, |_| Inner::with_capacity(per_shard));
        Self { capacity, sharded }
    }
}

impl<K, V> Cache<K, V> for LfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let new_age = inner.tick();

        let (old_priority, new_priority, value) = {
            let entry = inner.map.get_mut(key)?;
            let old = (entry.count, entry.age);
            entry.count = entry.count.saturating_add(1);
            entry.age = new_age;
            let new = (entry.count, entry.age);
            (old, new, entry.value.clone())
        };

        let _ = inner.by_priority.remove(&old_priority);
        let _ = inner.by_priority.insert(new_priority, key.clone());
        Some(value)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.sharded.shard_for(&key).lock_recover();
        let new_age = inner.tick();

        // Live update.
        if let Some(entry) = inner.map.get_mut(&key) {
            let old_priority = (entry.count, entry.age);
            entry.count = entry.count.saturating_add(1);
            entry.age = new_age;
            let new_priority = (entry.count, entry.age);
            let old_value = core::mem::replace(&mut entry.value, value);
            let _ = inner.by_priority.remove(&old_priority);
            let _ = inner.by_priority.insert(new_priority, key);
            return Some(old_value);
        }

        // New key — evict if at per-shard capacity.
        if inner.map.len() >= inner.capacity.get() {
            if let Some((_, victim_key)) = inner.by_priority.pop_first() {
                let _ = inner.map.remove(&victim_key);
            }
        }

        let entry = Entry {
            value,
            count: 1,
            age: new_age,
        };
        let priority = (entry.count, entry.age);
        let _ = inner.map.insert(key.clone(), entry);
        let _ = inner.by_priority.insert(priority, key);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let entry = inner.map.remove(key)?;
        let _ = inner.by_priority.remove(&(entry.count, entry.age));
        Some(entry.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.sharded
            .shard_for(key)
            .lock_recover()
            .map
            .contains_key(key)
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
            inner.map.clear();
            inner.by_priority.clear();
            inner.clock = 0;
        }
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}
