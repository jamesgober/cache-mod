//! Least-Frequently-Used (LFU) cache — BTreeMap-indexed O(log n) eviction.

use core::hash::Hash;
use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;
use std::sync::Mutex;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::util::MutexExt;

/// A bounded, thread-safe LFU cache.
///
/// Each entry carries a counter that is incremented on every [`get`](Cache::get)
/// or [`insert`](Cache::insert) of an already-present key. On overflow, the
/// entry with the **lowest counter** is evicted; ties are broken in favour of
/// evicting the **least-recently-accessed** entry.
///
/// [`contains_key`](Cache::contains_key) is a query and does not increment
/// the counter or touch access order, per the [`Cache`] contract.
///
/// 0.6.0 implementation: a `HashMap` for value lookup paired with a
/// `BTreeMap<(count, age), K>` ordered index. Every access and eviction is
/// O(log n) — the 0.5.x O(n) min-scan is gone. The trade-off is one extra
/// `K::clone()` per access, paid back many-fold once the cache holds more
/// than a few dozen entries. The sharded / lock-free lock-strategy upgrade
/// lands in 0.7.0 without changing this public surface.
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
/// // Bump "a"'s frequency above "b"'s.
/// assert_eq!(cache.get(&"a"), Some(1));
/// assert_eq!(cache.get(&"a"), Some(1));
///
/// // Inserting "c" should evict "b" (lowest counter).
/// cache.insert("c", 3);
/// assert_eq!(cache.get(&"b"), None);
/// assert_eq!(cache.get(&"a"), Some(1));
/// assert_eq!(cache.get(&"c"), Some(3));
/// ```
pub struct LfuCache<K, V> {
    capacity: NonZeroUsize,
    inner: Mutex<Inner<K, V>>,
}

struct Entry<V> {
    value: V,
    /// Number of accesses since insertion.
    count: u64,
    /// Monotonic access marker. Lower = older.
    age: u64,
}

struct Inner<K, V> {
    map: HashMap<K, Entry<V>>,
    /// Eviction priority index. Sorted by (count, age) — lowest first, so
    /// `pop_first` gives the least-frequently-used, breaking ties with
    /// least-recently-accessed.
    by_priority: BTreeMap<(u64, u64), K>,
    /// Monotonic clock used to stamp `Entry::age`. Wraps after 2^64 ops.
    clock: u64,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn with_capacity(cap: usize) -> Self {
        Self {
            map: HashMap::with_capacity(cap),
            by_priority: BTreeMap::new(),
            clock: 0,
        }
    }

    /// Advance the clock and return the new age value.
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
        let cap = capacity.get();
        Self {
            capacity,
            inner: Mutex::new(Inner::with_capacity(cap)),
        }
    }
}

impl<K, V> Cache<K, V> for LfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let new_age = inner.tick();

        // Extract old priority so we can update the BTreeMap without
        // double-borrowing.
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
        let mut inner = self.inner.lock_recover();
        let new_age = inner.tick();

        // Live update path.
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

        // New key — evict if at capacity.
        if inner.map.len() >= self.capacity.get() {
            if let Some((victim_priority, victim_key)) = inner.by_priority.pop_first() {
                let _ = inner.map.remove(&victim_key);
                // pop_first already removed the priority entry — nothing
                // more to do. Suppress unused.
                let _ = victim_priority;
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
        let mut inner = self.inner.lock_recover();
        let entry = inner.map.remove(key)?;
        let _ = inner.by_priority.remove(&(entry.count, entry.age));
        Some(entry.value)
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
        inner.by_priority.clear();
        inner.clock = 0;
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}
