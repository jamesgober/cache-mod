//! The [`Cache`] trait — the common contract every cache type in this crate
//! implements.

use core::hash::Hash;

/// The common read / write / evict contract every cache type in this crate
/// implements.
///
/// All methods take `&self` (not `&mut self`) so a cache instance can be
/// shared across threads and across `.await` points without external locking.
/// Implementations use interior mutability.
///
/// # Access semantics
///
/// - [`get`](Self::get) is an **access**: it may update the eviction order
///   (e.g. promoting the entry to most-recently-used).
/// - [`contains_key`](Self::contains_key) is a **query** only: it must not
///   update the eviction order.
/// - [`insert`](Self::insert) is an access on the inserted key.
/// - [`remove`](Self::remove) is destructive and does not update order.
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, LruCache};
///
/// let cache: LruCache<&'static str, u32> = LruCache::new(4).expect("capacity > 0");
///
/// assert_eq!(cache.insert("a", 1), None);
/// assert_eq!(cache.get(&"a"), Some(1));
/// assert!(cache.contains_key(&"a"));
/// assert_eq!(cache.len(), 1);
///
/// assert_eq!(cache.remove(&"a"), Some(1));
/// assert!(cache.is_empty());
/// ```
pub trait Cache<K, V>
where
    K: Eq + Hash,
    V: Clone,
{
    /// Returns the value associated with `key`, if any, and counts as an
    /// access for the purposes of the eviction policy.
    fn get(&self, key: &K) -> Option<V>;

    /// Inserts `value` under `key`. Returns the previously-stored value if
    /// `key` was already present.
    ///
    /// May evict one or more existing entries to make room, according to
    /// the cache's eviction policy.
    fn insert(&self, key: K, value: V) -> Option<V>;

    /// Removes the entry for `key` and returns the value if present.
    fn remove(&self, key: &K) -> Option<V>;

    /// Returns `true` if the cache currently holds an entry for `key`.
    ///
    /// Unlike [`get`](Self::get), this method does **not** count as an
    /// access — eviction order is left unchanged.
    fn contains_key(&self, key: &K) -> bool;

    /// Number of entries currently stored.
    fn len(&self) -> usize;

    /// Returns `true` when the cache holds no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every entry. Capacity is preserved.
    fn clear(&self);

    /// Configured maximum number of entries.
    fn capacity(&self) -> usize;
}
