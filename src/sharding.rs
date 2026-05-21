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

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap_or(NonZeroUsize::MIN)
    }

    #[test]
    fn shard_count_tiny_caches_use_one_shard() {
        for cap in [1usize, 2, 3, 4, 8, 16, 24, 31] {
            assert_eq!(
                shard_count(nz(cap)),
                1,
                "capacity {cap} should give 1 shard (below MIN_PER_SHARD * 2 = 32)",
            );
        }
    }

    #[test]
    fn shard_count_scales_with_capacity() {
        assert_eq!(shard_count(nz(32)), 2);
        assert_eq!(shard_count(nz(48)), 2);
        assert_eq!(shard_count(nz(64)), 4);
        assert_eq!(shard_count(nz(127)), 4);
        assert_eq!(shard_count(nz(128)), 8);
        assert_eq!(shard_count(nz(256)), 16);
        // Capped at DEFAULT_SHARDS even for very large caches.
        assert_eq!(shard_count(nz(1_000_000)), 16);
    }

    #[test]
    fn shard_count_is_always_power_of_two() {
        for cap in [1usize, 16, 32, 33, 64, 100, 256, 1024, 65_536] {
            assert!(
                shard_count(nz(cap)).is_power_of_two(),
                "shard_count({cap}) must be a power of two",
            );
        }
    }

    #[test]
    fn per_shard_capacity_distributes_evenly_when_divisible() {
        assert_eq!(per_shard_capacity(nz(64), 4).get(), 16);
        assert_eq!(per_shard_capacity(nz(256), 16).get(), 16);
        assert_eq!(per_shard_capacity(nz(1024), 16).get(), 64);
    }

    #[test]
    fn per_shard_capacity_floors_when_not_divisible() {
        // 17 / 16 = 1 (floor), so total effective capacity is 16 — one entry
        // is "lost" to integer division. Documented behaviour.
        assert_eq!(per_shard_capacity(nz(17), 16).get(), 1);
        // 100 / 8 = 12, total = 96 — 4 entries lost.
        assert_eq!(per_shard_capacity(nz(100), 8).get(), 12);
    }

    #[test]
    fn per_shard_capacity_never_returns_zero() {
        // Pathological case: capacity 1 with many shards. Per-shard is
        // floored to 1, never to 0.
        assert_eq!(per_shard_capacity(nz(1), 16).get(), 1);
    }

    #[test]
    fn from_factory_creates_requested_number_of_shards() {
        let sharded: Sharded<usize> = Sharded::from_factory(4, |i| i * 10);
        assert_eq!(sharded.iter().count(), 4);
        let values: Vec<usize> = sharded
            .iter()
            .map(|m| match m.lock() {
                Ok(g) => *g,
                Err(p) => *p.into_inner(),
            })
            .collect();
        assert_eq!(values, vec![0, 10, 20, 30]);
    }

    #[test]
    fn shard_for_routes_deterministically() {
        let sharded: Sharded<usize> = Sharded::from_factory(16, |_| 0);
        let key = "hello";
        let first = sharded.shard_for(key) as *const _;
        for _ in 0..32 {
            assert_eq!(sharded.shard_for(key) as *const _, first);
        }
    }

    #[test]
    fn shard_for_distributes_keys_across_shards() {
        let sharded: Sharded<usize> = Sharded::from_factory(16, |_| 0);
        let mut distinct = std::collections::HashSet::new();
        for i in 0..1024u32 {
            let _ = distinct.insert(sharded.shard_for(&i) as *const _ as usize);
        }
        // Statistical: at least 8 of 16 shards must be hit by 1024 distinct keys.
        assert!(
            distinct.len() >= 8,
            "expected at least 8 distinct shards across 1024 keys, hit {}",
            distinct.len(),
        );
    }
}
