//! Least-Frequently-Used (LFU) cache.

use core::hash::Hash;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Mutex, MutexGuard};

use crate::cache::Cache;
use crate::error::CacheError;

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
/// This is the 0.3.0 reference implementation: correct and `&self`-everywhere,
/// `Mutex`-guarded. Eviction is O(n) — a scan for the minimum on overflow.
/// An O(1) bucket-based implementation lands in 0.5.0 without changing this
/// public surface.
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
    /// Number of accesses (`get` + `insert`-of-existing-key) since insertion.
    count: u64,
    /// Monotonic access marker; updated on every access. Lower = older.
    /// Tie-break secondary criterion when multiple entries share `count`.
    last_access: u64,
}

struct Inner<K, V> {
    map: HashMap<K, Entry<V>>,
    /// Monotonic counter used to stamp `Entry::last_access`. Wraps with
    /// `wrapping_add`; long-running caches see one collision per 2^64 ops,
    /// which is acceptable because tie-breaking is a best-effort secondary
    /// criterion already.
    clock: u64,
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
            inner: Mutex::new(Inner {
                map: HashMap::with_capacity(cap),
                clock: 0,
            }),
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, Inner<K, V>> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl<K, V> Cache<K, V> for LfuCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.lock_inner();
        inner.clock = inner.clock.wrapping_add(1);
        let now = inner.clock;
        let entry = inner.map.get_mut(key)?;
        entry.count = entry.count.saturating_add(1);
        entry.last_access = now;
        Some(entry.value.clone())
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.lock_inner();
        inner.clock = inner.clock.wrapping_add(1);
        let now = inner.clock;

        if let Some(existing) = inner.map.get_mut(&key) {
            let old = core::mem::replace(&mut existing.value, value);
            existing.count = existing.count.saturating_add(1);
            existing.last_access = now;
            return Some(old);
        }

        // New key — evict if at capacity.
        if inner.map.len() >= self.capacity.get() {
            if let Some(victim) = find_victim(&inner.map) {
                let _ = inner.map.remove(&victim);
            }
        }

        let _ = inner.map.insert(
            key,
            Entry {
                value,
                count: 1,
                last_access: now,
            },
        );
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.lock_inner();
        inner.map.remove(key).map(|e| e.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.lock_inner().map.contains_key(key)
    }

    fn len(&self) -> usize {
        self.lock_inner().map.len()
    }

    fn clear(&self) {
        let mut inner = self.lock_inner();
        inner.map.clear();
        inner.clock = 0;
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

/// Eviction target: minimum `count`, ties broken by minimum `last_access`
/// (least-recently-accessed).
fn find_victim<K, V>(map: &HashMap<K, Entry<V>>) -> Option<K>
where
    K: Clone,
{
    map.iter()
        .min_by(|(_, a), (_, b)| {
            a.count
                .cmp(&b.count)
                .then(a.last_access.cmp(&b.last_access))
        })
        .map(|(k, _)| k.clone())
}
