//! Internal sharding helper used by every cache type.
//!
//! Splits a logical cache into N independent shards routed by hash of the
//! cache key. Each shard owns its own arena / map / priority structure, so
//! contention on the inner `Mutex` is bounded by the number of threads
//! hashing into the same shard.
//!
//! Nothing in this module is part of the public API.
//!
//! # Behavioral consequence
//!
//! With more than one shard, every cache type's eviction policy is
//! **approximate**: the entry evicted on overflow is the local-to-shard
//! least-recently-used / least-frequently-used / soonest-expiring, not the
//! global one. This is the same trade-off DashMap, moka, and caffeine make.
//! Tiny caches (< `MIN_PER_SHARD * 2` entries) use a single shard, so they
//! retain the strict global ordering documented at the type level.

use core::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::num::NonZeroUsize;
use std::sync::Mutex;

/// Default maximum shard count. Capped at 16 because each additional shard
/// adds bookkeeping overhead with diminishing returns once the cache is
/// already sharded enough to absorb typical thread counts.
pub(crate) const DEFAULT_SHARDS: usize = 16;

/// Minimum per-shard capacity. Below this, sharding is counter-productive —
/// the per-shard overhead dominates and approximate-LRU loses too much
/// fidelity for too little contention reduction.
pub(crate) const MIN_PER_SHARD: usize = 16;

/// Picks a shard count for the given total capacity. Always a power of two
/// so `hash & (shards - 1)` works in place of `hash % shards`.
///
/// Returns 1 (no sharding) for caches smaller than `MIN_PER_SHARD * 2`,
/// which keeps tiny caches (and most tests) in strict global-eviction-order
/// mode without changing behavior.
pub(crate) fn shard_count(capacity: NonZeroUsize) -> usize {
    let cap = capacity.get();
    if cap < MIN_PER_SHARD.saturating_mul(2) {
        return 1;
    }
    let max_by_cap = cap / MIN_PER_SHARD;
    let mut shards = 1usize;
    while shards.saturating_mul(2) <= max_by_cap && shards.saturating_mul(2) <= DEFAULT_SHARDS {
        shards = shards.saturating_mul(2);
    }
    shards
}

/// Per-shard capacity, computed by floor division of the total. The total
/// effective capacity is `num_shards * per_shard_capacity`, which may be
/// slightly less than the requested `capacity` when it's not a multiple of
/// `num_shards`. The discrepancy is at most `num_shards - 1` entries.
pub(crate) fn per_shard_capacity(capacity: NonZeroUsize, num_shards: usize) -> NonZeroUsize {
    let per = (capacity.get() / num_shards).max(1);
    NonZeroUsize::new(per).unwrap_or(NonZeroUsize::MIN)
}

/// A sharded `Mutex<T>` collection. `T` is each cache's `Inner` state.
/// Shards are independent — no cross-shard coordination is performed.
pub(crate) struct Sharded<T> {
    shards: Box<[Mutex<T>]>,
    /// `shards.len() - 1`. `shards.len()` is a power of two; the mask
    /// lets us replace `% num_shards` with a single bitwise AND.
    shard_mask: usize,
}

impl<T> Sharded<T> {
    /// Build a `Sharded<T>` of length `num_shards` by calling `factory(i)`
    /// for each shard index `i`. Caller must ensure `num_shards` is a power
    /// of two and at least 1.
    pub(crate) fn from_factory<F>(num_shards: usize, mut factory: F) -> Self
    where
        F: FnMut(usize) -> T,
    {
        debug_assert!(num_shards.is_power_of_two() && num_shards >= 1);
        let shards: Box<[Mutex<T>]> = (0..num_shards)
            .map(|i| Mutex::new(factory(i)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let shard_mask = num_shards - 1;
        Self { shards, shard_mask }
    }

    /// Route `key` to its shard. Uses `DefaultHasher` — the hash is only
    /// used internally for sharding, never exposed.
    pub(crate) fn shard_for<K: Hash + ?Sized>(&self, key: &K) -> &Mutex<T> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let idx = (hasher.finish() as usize) & self.shard_mask;
        &self.shards[idx]
    }

    /// Iterator over every shard. Used for cross-shard ops like `len` and
    /// `clear`.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Mutex<T>> {
        self.shards.iter()
    }
}
