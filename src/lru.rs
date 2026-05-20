//! Least-Recently-Used (LRU) cache — the reference implementation of
//! [`Cache`](crate::Cache).

use core::hash::Hash;
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Mutex;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::util::MutexExt;

/// A bounded, thread-safe LRU cache.
///
/// On insert overflow the least-recently-accessed entry is evicted. Both
/// [`get`](Cache::get) and [`insert`](Cache::insert) count as accesses and
/// promote the affected entry to most-recently-used.
///
/// This is the reference implementation: correct, `&self`-everywhere,
/// `Mutex`-guarded. A lock-free, arena-backed implementation lands in a
/// later minor without changing this public surface.
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, LruCache};
///
/// let cache: LruCache<u32, &'static str> = LruCache::new(2).expect("capacity > 0");
///
/// cache.insert(1, "one");
/// cache.insert(2, "two");
/// assert_eq!(cache.get(&1), Some("one")); // 1 is now MRU, 2 is LRU
///
/// cache.insert(3, "three"); // evicts 2
/// assert_eq!(cache.get(&2), None);
/// assert_eq!(cache.get(&1), Some("one"));
/// assert_eq!(cache.get(&3), Some("three"));
/// ```
pub struct LruCache<K, V> {
    capacity: NonZeroUsize,
    inner: Mutex<Inner<K, V>>,
}

struct Inner<K, V> {
    /// Storage of live entries.
    map: HashMap<K, V>,
    /// Access order. Front = most-recently-used, back = least-recently-used.
    order: VecDeque<K>,
}

impl<K, V> LruCache<K, V>
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
    /// use cache_mod::LruCache;
    ///
    /// let cache: LruCache<String, u32> = LruCache::new(128).expect("capacity > 0");
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
    /// use cache_mod::LruCache;
    ///
    /// let cap = NonZeroUsize::new(64).expect("64 != 0");
    /// let cache: LruCache<String, u32> = LruCache::with_capacity(cap);
    /// ```
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        let cap = capacity.get();
        Self {
            capacity,
            inner: Mutex::new(Inner {
                map: HashMap::with_capacity(cap),
                order: VecDeque::with_capacity(cap),
            }),
        }
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let value = inner.map.get(key)?.clone();
        promote(&mut inner.order, key);
        Some(value)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let old = inner.map.insert(key.clone(), value);
        if old.is_some() {
            promote(&mut inner.order, &key);
        } else {
            inner.order.push_front(key);
            while inner.order.len() > self.capacity.get() {
                if let Some(evicted) = inner.order.pop_back() {
                    let _ = inner.map.remove(&evicted);
                }
            }
        }
        old
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let value = inner.map.remove(key)?;
        if let Some(pos) = inner.order.iter().position(|k| k == key) {
            let _ = inner.order.remove(pos);
        }
        Some(value)
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
        inner.order.clear();
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

/// Moves `key` to the front of `order` if it is present. No-op otherwise.
fn promote<K: Eq>(order: &mut VecDeque<K>, key: &K) {
    if let Some(pos) = order.iter().position(|k| k == key) {
        if let Some(k) = order.remove(pos) {
            order.push_front(k);
        }
    }
}
