//! Time-To-Live (TTL) cache — sharded, lazy-expiry reference implementation.

use core::hash::Hash;
use std::collections::HashMap;
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
/// # Implementation (0.7.0)
///
/// Sharded into up to 16 independent stores keyed by hash of `K`. Each
/// shard owns its own `HashMap` and applies expiry / overflow eviction
/// locally. Tiny caches (< 32 entries) use a single shard.
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
}

struct Inner<K, V> {
    capacity: NonZeroUsize,
    map: HashMap<K, Entry<V>>,
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

    fn insert_inner(inner: &mut Inner<K, V>, key: K, value: V, deadline: Instant) -> Option<V> {
        let now = Instant::now();

        if let Some(existing) = inner.map.get_mut(&key) {
            if existing.expires_at > now {
                let old = core::mem::replace(&mut existing.value, value);
                existing.expires_at = deadline;
                return Some(old);
            }
        }

        let _ = inner.map.remove(&key);

        if inner.map.len() >= inner.capacity.get() {
            if let Some(victim) = find_victim(&inner.map) {
                let _ = inner.map.remove(&victim);
            }
        }

        let _ = inner.map.insert(
            key,
            Entry {
                value,
                expires_at: deadline,
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
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let now = Instant::now();
        let expires_at = inner.map.get(key).map(|e| e.expires_at)?;
        if expires_at <= now {
            let _ = inner.map.remove(key);
            return None;
        }
        inner.map.get(key).map(|e| e.value.clone())
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let deadline = compute_deadline(self.default_ttl);
        let mut inner = self.sharded.shard_for(&key).lock_recover();
        Self::insert_inner(&mut inner, key, value, deadline)
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        inner.map.remove(key).map(|e| e.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let now = Instant::now();
        let Some(expires_at) = inner.map.get(key).map(|e| e.expires_at) else {
            return false;
        };
        if expires_at > now {
            return true;
        }
        let _ = inner.map.remove(key);
        false
    }

    fn len(&self) -> usize {
        let mut total = 0;
        for mutex in self.sharded.iter() {
            let mut inner = mutex.lock_recover();
            purge_expired(&mut inner.map);
            total += inner.map.len();
        }
        total
    }

    fn clear(&self) {
        for mutex in self.sharded.iter() {
            let mut inner = mutex.lock_recover();
            inner.map.clear();
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

fn find_victim<K, V>(map: &HashMap<K, Entry<V>>) -> Option<K>
where
    K: Clone,
{
    map.iter()
        .min_by_key(|(_, e)| e.expires_at)
        .map(|(k, _)| k.clone())
}

fn purge_expired<K, V>(map: &mut HashMap<K, Entry<V>>) {
    let now = Instant::now();
    map.retain(|_, entry| entry.expires_at > now);
}
