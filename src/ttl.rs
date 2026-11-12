//! Time-To-Live (TTL) cache — sharded, lazy-expiry reference implementation.

use core::borrow::Borrow;
use core::hash::Hash;
use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use crate::cache::Cache;
use crate::error::CacheError;
use crate::sharding::{self, Sharded};
use crate::util::MutexExt;

/// Fallback deadline span (~100 years) used when `Instant + ttl` would
/// overflow. No realistic cache lifetime approaches this.
const FAR_FUTURE: Duration = Duration::from_secs(60 * 60 * 24 * 365 * 100);

/// A bounded, thread-safe cache with per-entry time-to-live.
///
/// Each entry is stamped with a deadline at insert time. On every access
/// ([`get`](Cache::get), [`contains_key`](Cache::contains_key),
/// [`len`](Cache::len)), expired entries are removed lazily. On overflow,
/// the entry with the **soonest expiration** is evicted — already-expired
/// entries are naturally preferred over live ones.
///
/// Both [`insert`](Cache::insert) and [`insert_with_ttl`](TtlCache::insert_with_ttl)
/// reset the deadline on the affected entry — writes always re-arm the timer.
///
/// # Implementation
///
/// Sharded into up to 16 independent stores keyed by hash of `K`. Each
/// shard owns its own `HashMap` plus a deadline index ordered by expiry
/// time, and applies expiry / overflow eviction locally. Tiny caches
/// (< 32 entries) use a single shard.
///
/// # Cost
///
/// `get`, `insert`, `insert_with_ttl`, `remove` and `contains_key` are
/// O(log n) in the shard size: finding the soonest-expiring entry on
/// overflow reads the front of the deadline index instead of scanning
/// the shard. [`len`](Cache::len) first drops every expired entry, at
/// O(log n) per expired entry, and then sums the shard sizes.
///
/// Eviction is **per-shard approximate** — overflow inside one shard
/// evicts the soonest-expiring entry within that shard, not necessarily
/// the soonest-expiring entry globally. Lazy expiry remains exact within
/// the operating shard.
///
/// # Example
///
/// ```
/// use std::time::Duration;
/// use cache_mod::{Cache, TtlCache};
///
/// let cache: TtlCache<&'static str, u32> =
///     TtlCache::new(4, Duration::from_secs(60)).expect("capacity > 0");
///
/// cache.insert("session", 42);
/// assert_eq!(cache.get(&"session"), Some(42));
/// ```
pub struct TtlCache<K, V> {
    capacity: NonZeroUsize,
    default_ttl: Duration,
    sharded: Sharded<Inner<K, V>>,
}

struct Entry<V> {
    value: V,
    expires_at: Instant,
    /// Tie-breaker that makes the deadline index key unique.
    seq: u64,
}

struct Inner<K, V> {
    capacity: NonZeroUsize,
    map: HashMap<K, Entry<V>>,
    /// Deadline index: `(expires_at, seq) -> key`, soonest first. Holds
    /// exactly one row per entry in `map`.
    by_deadline: BTreeMap<(Instant, u64), K>,
    next_seq: u64,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            map: HashMap::with_capacity(sharding::initial_reserve(capacity)),
            by_deadline: BTreeMap::new(),
            next_seq: 0,
        }
    }

    fn seq(&mut self) -> u64 {
        self.next_seq = self.next_seq.wrapping_add(1);
        self.next_seq
    }

    /// Remove `key` and its index row. Returns the entry if it was present.
    fn remove_entry<Q>(&mut self, key: &Q) -> Option<Entry<V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let entry = self.map.remove(key)?;
        let _ = self.by_deadline.remove(&(entry.expires_at, entry.seq));
        Some(entry)
    }

    /// Evict the soonest-expiring entry.
    fn evict_soonest(&mut self) {
        if let Some((_, victim)) = self.by_deadline.pop_first() {
            let _ = self.map.remove(&victim);
        }
    }

    /// Drop every entry whose deadline is at or before `now`.
    fn purge_expired(&mut self, now: Instant) {
        while let Some((&(deadline, _), _)) = self.by_deadline.first_key_value() {
            if deadline > now {
                break;
            }
            self.evict_soonest();
        }
    }

    /// Look up a live entry, dropping it if it has expired.
    fn live<Q>(&mut self, key: &Q, now: Instant) -> Option<&Entry<V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let expires_at = self.map.get(key)?.expires_at;
        if expires_at <= now {
            let _ = self.remove_entry(key);
            return None;
        }
        self.map.get(key)
    }

    /// Assert the internal invariants. Test builds only.
    #[cfg(test)]
    fn check_invariants(&self) {
        assert_eq!(
            self.map.len(),
            self.by_deadline.len(),
            "one index row per entry"
        );
        for (&(deadline, seq), key) in &self.by_deadline {
            let entry = match self.map.get(key) {
                Some(e) => e,
                None => panic!("index row without entry"),
            };
            assert_eq!(
                (entry.expires_at, entry.seq),
                (deadline, seq),
                "row matches entry"
            );
        }
    }
}

impl<K, V> TtlCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a cache with the given capacity and default time-to-live.
    ///
    /// `ttl` is applied to every `insert` that does not specify its own.
    /// Returns [`CacheError::InvalidCapacity`] if `capacity == 0`.
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::Duration;
    /// use cache_mod::TtlCache;
    ///
    /// let cache: TtlCache<String, u32> =
    ///     TtlCache::new(128, Duration::from_secs(300)).expect("capacity > 0");
    /// ```
    pub fn new(capacity: usize, ttl: Duration) -> Result<Self, CacheError> {
        let cap = NonZeroUsize::new(capacity).ok_or(CacheError::InvalidCapacity)?;
        Ok(Self::with_capacity(cap, ttl))
    }

    /// Creates a cache with the given non-zero capacity and default TTL.
    /// Infallible.
    ///
    /// # Example
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use std::time::Duration;
    /// use cache_mod::TtlCache;
    ///
    /// let cap = NonZeroUsize::new(64).expect("64 != 0");
    /// let cache: TtlCache<String, u32> =
    ///     TtlCache::with_capacity(cap, Duration::from_secs(60));
    /// ```
    pub fn with_capacity(capacity: NonZeroUsize, ttl: Duration) -> Self {
        let num_shards = sharding::shard_count(capacity);
        let per_shard = sharding::per_shard_capacity(capacity, num_shards);
        let sharded = Sharded::from_factory(num_shards, |_| Inner::with_capacity(per_shard));
        Self {
            capacity,
            default_ttl: ttl,
            sharded,
        }
    }

    /// Inserts `value` under `key` with a per-call TTL that overrides the
    /// cache default. The deadline is `now + ttl`.
    ///
    /// Returns the previously-stored **live** value if `key` was already
    /// present and not yet expired. An expired-but-not-yet-cleaned entry
    /// is treated as absent: the call returns `None` and replaces it.
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::Duration;
    /// use cache_mod::{Cache, TtlCache};
    ///
    /// let cache: TtlCache<u32, u32> =
    ///     TtlCache::new(4, Duration::from_secs(60)).expect("capacity > 0");
    ///
    /// cache.insert_with_ttl(1, 10, Duration::from_secs(5));
    /// assert_eq!(cache.get(&1), Some(10));
    /// ```
    pub fn insert_with_ttl(&self, key: K, value: V, ttl: Duration) -> Option<V> {
        let deadline = compute_deadline(ttl);
        let mut inner = self.sharded.shard_for(&key).lock_recover();
        Self::insert_inner(&mut inner, key, value, deadline)
    }

    /// Like [`get`](Cache::get), but takes any borrowed form of the key,
    /// so a `TtlCache<String, V>` can be queried with a `&str` without
    /// building a `String`.
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::Duration;
    /// use cache_mod::{Cache, TtlCache};
    ///
    /// let cache: TtlCache<String, u32> =
    ///     TtlCache::new(8, Duration::from_secs(60)).expect("capacity > 0");
    /// cache.insert("session".to_string(), 7);
    /// assert_eq!(cache.get_by("session"), Some(7));
    /// ```
    pub fn get_by<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        inner.live(key, Instant::now()).map(|e| e.value.clone())
    }

    /// Like [`contains_key`](Cache::contains_key), but takes any borrowed
    /// form of the key. An expired entry is removed and reported absent.
    pub fn contains_key_by<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        inner.live(key, Instant::now()).is_some()
    }

    /// Like [`remove`](Cache::remove), but takes any borrowed form of the
    /// key.
    pub fn remove_by<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        inner.remove_entry(key).map(|e| e.value)
    }

    fn insert_inner(inner: &mut Inner<K, V>, key: K, value: V, deadline: Instant) -> Option<V> {
        let now = Instant::now();

        // Live update: replace the value and move the index row to the
        // new deadline, reusing the stored key.
        if let Some(existing) = inner.map.get_mut(&key) {
            if existing.expires_at > now {
                let old = core::mem::replace(&mut existing.value, value);
                let old_row = (existing.expires_at, existing.seq);
                existing.expires_at = deadline;
                let seq = existing.seq;
                if let Some(stored) = inner.by_deadline.remove(&old_row) {
                    let _ = inner.by_deadline.insert((deadline, seq), stored);
                }
                return Some(old);
            }
        }

        let _ = inner.remove_entry(&key);

        if inner.map.len() >= inner.capacity.get() {
            inner.evict_soonest();
        }

        let seq = inner.seq();
        let _ = inner.by_deadline.insert((deadline, seq), key.clone());
        let _ = inner.map.insert(
            key,
            Entry {
                value,
                expires_at: deadline,
                seq,
            },
        );
        None
    }
}

impl<K, V> Cache<K, V> for TtlCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        self.get_by(key)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let deadline = compute_deadline(self.default_ttl);
        let mut inner = self.sharded.shard_for(&key).lock_recover();
        Self::insert_inner(&mut inner, key, value, deadline)
    }

    fn remove(&self, key: &K) -> Option<V> {
        self.remove_by(key)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.contains_key_by(key)
    }

    fn len(&self) -> usize {
        let now = Instant::now();
        let mut total = 0;
        for mutex in self.sharded.iter() {
            let mut inner = mutex.lock_recover();
            inner.purge_expired(now);
            total += inner.map.len();
        }
        total
    }

    fn clear(&self) {
        for mutex in self.sharded.iter() {
            let mut inner = mutex.lock_recover();
            inner.map.clear();
            inner.by_deadline.clear();
        }
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

fn compute_deadline(ttl: Duration) -> Instant {
    let now = Instant::now();
    match now.checked_add(ttl) {
        Some(t) => t,
        None => now.checked_add(FAR_FUTURE).unwrap_or(now),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    impl<K: Eq + Hash + Clone, V> TtlCache<K, V> {
        fn check(&self) {
            for m in self.sharded.iter() {
                m.lock_recover().check_invariants();
            }
        }
    }

    #[test]
    fn overflow_evicts_soonest_deadline() {
        let c: TtlCache<u32, u32> = TtlCache::new(3, Duration::from_secs(60)).unwrap();
        let _ = c.insert_with_ttl(1, 1, Duration::from_secs(30));
        let _ = c.insert_with_ttl(2, 2, Duration::from_secs(10));
        let _ = c.insert_with_ttl(3, 3, Duration::from_secs(50));
        // Re-arming 2 moves it behind 1 in the deadline order.
        let _ = c.insert_with_ttl(2, 20, Duration::from_secs(40));
        let _ = c.insert(4, 4);
        c.check();
        assert!(!c.contains_key(&1));
        assert_eq!(c.get(&2), Some(20));
        assert!(c.contains_key(&3));
        assert!(c.contains_key(&4));
    }

    #[test]
    fn expired_entries_are_purged_through_the_index() {
        let c: TtlCache<u32, u32> = TtlCache::new(8, Duration::from_secs(60)).unwrap();
        let _ = c.insert_with_ttl(1, 1, Duration::ZERO);
        let _ = c.insert_with_ttl(2, 2, Duration::ZERO);
        let _ = c.insert(3, 3);
        assert_eq!(c.len(), 1);
        c.check();
        assert_eq!(c.get(&1), None);
        let _ = c.insert(1, 10);
        c.check();
        assert_eq!(c.get(&1), Some(10));
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u8, bool),
        Get(u8),
        Remove(u8),
        Contains(u8),
        Len,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => (0u8..24, any::<bool>()).prop_map(|(k, e)| Op::Insert(k, e)),
            2 => (0u8..24).prop_map(Op::Get),
            1 => (0u8..24).prop_map(Op::Remove),
            1 => (0u8..24).prop_map(Op::Contains),
            1 => Just(Op::Len),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn index_matches_map(ops in vec(op(), 1..200)) {
            let c: TtlCache<u8, u8> = TtlCache::new(8, Duration::from_secs(60)).unwrap();
            for op in ops {
                match op {
                    Op::Insert(k, expired) => {
                        let ttl = if expired { Duration::ZERO } else { Duration::from_secs(60) };
                        let _ = c.insert_with_ttl(k, k, ttl);
                    }
                    Op::Get(k) => {
                        if let Some(v) = c.get(&k) { prop_assert_eq!(v, k); }
                    }
                    Op::Remove(k) => { let _ = c.remove(&k); }
                    Op::Contains(k) => { let _ = c.contains_key(&k); }
                    Op::Len => { prop_assert!(c.len() <= 8); }
                }
                c.check();
            }
        }
    }
}
