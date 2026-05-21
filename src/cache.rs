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
    #[must_use = "the value lookup is the whole reason to call `get`; dropping it is almost certainly a bug"]
    fn get(&self, key: &K) -> Option<V>;

    /// Inserts `value` under `key`. Returns the previously-stored value if
    /// `key` was already present.
    ///
    /// May evict one or more existing entries to make room, according to
    /// the cache's eviction policy. For `TinyLfuCache` the value may also
    /// be silently rejected by the admission filter — in that case the
    /// return value is `None` and the cache is unchanged.
    ///
    /// The return value carries useful information (new-vs-replace, or
    /// admit-vs-reject for `TinyLfuCache`). If you genuinely don't need
    /// it, bind to `_` explicitly.
    fn insert(&self, key: K, value: V) -> Option<V>;

    /// Removes the entry for `key` and returns the value if present.
    fn remove(&self, key: &K) -> Option<V>;

    /// Returns `true` if the cache currently holds an entry for `key`.
    ///
    /// Unlike [`get`](Self::get), this method does **not** count as an
    /// access — eviction order is left unchanged. For `TtlCache` an
    /// expired-but-not-yet-cleaned entry is removed during the check and
    /// the method returns `false`.
    #[must_use = "ignoring `contains_key` defeats its purpose; use `_` to drop it explicitly"]
    fn contains_key(&self, key: &K) -> bool;

    /// Number of entries currently stored.
    ///
    /// For sharded caches (capacity ≥ 32), this is computed by summing
    /// each shard's length while briefly locking each in turn — it is
    /// **not** an atomic snapshot of all shards simultaneously. In
    /// practice this matters only for code that races a `len()` against
    /// concurrent writers and expects a single instantaneous value.
    #[must_use = "ignoring `len` defeats its purpose; use `_` to drop it explicitly"]
    fn len(&self) -> usize;

    /// Returns `true` when the cache holds no entries.
    #[must_use = "ignoring `is_empty` defeats its purpose; use `_` to drop it explicitly"]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every entry. Capacity is preserved.
    ///
    /// For caches with auxiliary state — `LfuCache`'s priority index,
    /// `TinyLfuCache`'s Count-Min Sketch, the monotonic clocks used by
    /// `LfuCache` and `TinyLfuCache` — that state is reset alongside the
    /// entries themselves. Configured capacity / `max_weight` is the only
    /// piece of state that survives.
    fn clear(&self);

    /// Configured capacity bound.
    ///
    /// The unit depends on the implementation:
    /// - `LruCache`, `LfuCache`, `TtlCache`, `TinyLfuCache` — maximum number of entries.
    /// - `SizedCache` — maximum total byte-weight across entries.
    #[must_use = "ignoring `capacity` defeats its purpose; use `_` to drop it explicitly"]
    fn capacity(&self) -> usize;
}
