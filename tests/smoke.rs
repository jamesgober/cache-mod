//! End-to-end smoke tests for the 0.2.0 public surface.
//!
//! These verify that `Cache`, `LruCache`, and `CacheError` are wired up
//! correctly and behave per their documented contracts.
//!
//! Gated on the `std` feature because `LruCache` is `std`-only in 0.2.0.

#![cfg(feature = "std")]

use std::time::Duration;

use cache_mod::{Cache, CacheError, LfuCache, LruCache, SizedCache, TinyLfuCache, TtlCache};

#[test]
fn version_is_set() {
    assert!(!cache_mod::VERSION.is_empty());
}

#[test]
fn zero_capacity_is_rejected() {
    let err = LruCache::<u32, u32>::new(0).err();
    assert_eq!(err, Some(CacheError::InvalidCapacity));
}

#[test]
fn insert_then_get_returns_value() {
    let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.len(), 1);
    assert!(!cache.is_empty());
}

#[test]
fn insert_replaces_existing_value() {
    let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.insert(1, 20), Some(10));
    assert_eq!(cache.get(&1), Some(20));
    assert_eq!(cache.len(), 1);
}

#[test]
fn lru_evicts_least_recently_used() {
    let cache: LruCache<u32, u32> = LruCache::new(2).expect("capacity > 0");

    cache.insert(1, 10);
    cache.insert(2, 20);
    // Access 1 -> 1 becomes MRU, 2 becomes LRU.
    assert_eq!(cache.get(&1), Some(10));

    // Inserting 3 should evict 2.
    cache.insert(3, 30);
    assert_eq!(cache.get(&2), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.get(&3), Some(30));
    assert_eq!(cache.len(), 2);
}

#[test]
fn contains_key_does_not_promote() {
    let cache: LruCache<u32, u32> = LruCache::new(2).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);

    // `contains_key` must NOT update access order. 1 should still be LRU.
    assert!(cache.contains_key(&1));

    // Now inserting 3 should evict 1 (still the LRU).
    cache.insert(3, 30);
    assert!(!cache.contains_key(&1));
    assert!(cache.contains_key(&2));
    assert!(cache.contains_key(&3));
}

#[test]
fn remove_returns_value_and_shrinks() {
    let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);

    assert_eq!(cache.remove(&1), Some(10));
    assert_eq!(cache.remove(&1), None);
    assert!(!cache.contains_key(&1));
    assert_eq!(cache.len(), 1);
}

#[test]
fn clear_empties_the_cache() {
    let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    cache.insert(3, 30);
    assert_eq!(cache.len(), 3);

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.capacity(), 4); // capacity preserved
    assert_eq!(cache.get(&1), None);
}

#[test]
fn capacity_reports_configured_size() {
    let cache: LruCache<u32, u32> = LruCache::new(32).expect("capacity > 0");
    assert_eq!(cache.capacity(), 32);
}

#[test]
fn cache_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<LruCache<u32, String>>();
    assert_sync::<LruCache<u32, String>>();
}

#[test]
fn error_display_is_actionable() {
    let msg = format!("{}", CacheError::InvalidCapacity);
    assert!(msg.contains("capacity"));
}

// -----------------------------------------------------------------------------
// LfuCache (0.3.0)
// -----------------------------------------------------------------------------

#[test]
fn lfu_zero_capacity_is_rejected() {
    let err = LfuCache::<u32, u32>::new(0).err();
    assert_eq!(err, Some(CacheError::InvalidCapacity));
}

#[test]
fn lfu_insert_replaces_existing_value() {
    let cache: LfuCache<u32, u32> = LfuCache::new(4).expect("capacity > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.insert(1, 20), Some(10));
    assert_eq!(cache.get(&1), Some(20));
    assert_eq!(cache.len(), 1);
}

#[test]
fn lfu_evicts_least_frequently_used() {
    let cache: LfuCache<u32, u32> = LfuCache::new(2).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);

    // Bump key 1's counter above key 2's.
    assert_eq!(cache.get(&1), Some(10));

    // Inserting 3 must evict 2 (lowest counter).
    cache.insert(3, 30);
    assert_eq!(cache.get(&2), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.get(&3), Some(30));
    assert_eq!(cache.len(), 2);
}

#[test]
fn lfu_tie_breaks_with_least_recently_accessed() {
    let cache: LfuCache<u32, u32> = LfuCache::new(2).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    // Both keys at count = 1. Key 1 was inserted first and never re-accessed,
    // so it is the least-recently-accessed entry.

    cache.insert(3, 30); // must evict key 1
    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2), Some(20));
    assert_eq!(cache.get(&3), Some(30));
}

#[test]
fn lfu_contains_key_does_not_promote() {
    let cache: LfuCache<u32, u32> = LfuCache::new(2).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);

    // contains_key MUST NOT touch frequency or access order — key 1 must
    // remain the LRU tie-break victim.
    assert!(cache.contains_key(&1));
    assert!(cache.contains_key(&1));
    assert!(cache.contains_key(&1));

    cache.insert(3, 30);
    assert!(!cache.contains_key(&1));
    assert!(cache.contains_key(&2));
    assert!(cache.contains_key(&3));
}

#[test]
fn lfu_remove_returns_value_and_shrinks() {
    let cache: LfuCache<u32, u32> = LfuCache::new(4).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);

    assert_eq!(cache.remove(&1), Some(10));
    assert_eq!(cache.remove(&1), None);
    assert!(!cache.contains_key(&1));
    assert_eq!(cache.len(), 1);
}

#[test]
fn lfu_clear_empties_the_cache() {
    let cache: LfuCache<u32, u32> = LfuCache::new(4).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    cache.insert(3, 30);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.capacity(), 4);
    assert_eq!(cache.get(&1), None);
}

#[test]
fn lfu_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<LfuCache<u32, String>>();
    assert_sync::<LfuCache<u32, String>>();
}

// -----------------------------------------------------------------------------
// TtlCache (0.4.0)
// -----------------------------------------------------------------------------

/// TTL used by tests that want an entry to be expired by the time we touch
/// it. Paired with a `thread::sleep` of `EXPIRY_SLEEP`, which is ~10× the TTL.
const SHORT_TTL: Duration = Duration::from_millis(1);
const EXPIRY_SLEEP: Duration = Duration::from_millis(10);

/// A TTL long enough that no test in this file will see it expire.
const LONG_TTL: Duration = Duration::from_secs(60);

#[test]
fn ttl_zero_capacity_is_rejected() {
    let err = TtlCache::<u32, u32>::new(0, LONG_TTL).err();
    assert_eq!(err, Some(CacheError::InvalidCapacity));
}

#[test]
fn ttl_insert_then_get_within_window() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, LONG_TTL).expect("capacity > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.len(), 1);
}

#[test]
fn ttl_get_returns_none_for_expired_entry() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, SHORT_TTL).expect("capacity > 0");
    cache.insert(1, 10);
    std::thread::sleep(EXPIRY_SLEEP);
    assert_eq!(cache.get(&1), None);
    // contains_key also reports false after the get-driven cleanup.
    assert!(!cache.contains_key(&1));
}

#[test]
fn ttl_contains_key_clears_expired_lazily() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, SHORT_TTL).expect("capacity > 0");
    cache.insert(1, 10);
    assert!(cache.contains_key(&1));
    std::thread::sleep(EXPIRY_SLEEP);
    assert!(!cache.contains_key(&1));
}

#[test]
fn ttl_len_purges_expired() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, SHORT_TTL).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    std::thread::sleep(EXPIRY_SLEEP);
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn ttl_per_call_override_takes_precedence_over_default() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, LONG_TTL).expect("capacity > 0");
    cache.insert_with_ttl(1, 10, SHORT_TTL);
    cache.insert(2, 20); // default = LONG_TTL
    std::thread::sleep(EXPIRY_SLEEP);
    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2), Some(20));
}

#[test]
fn ttl_eviction_prefers_soonest_expiring() {
    let cache: TtlCache<u32, u32> = TtlCache::new(2, LONG_TTL).expect("capacity > 0");
    cache.insert_with_ttl(1, 10, Duration::from_secs(60));
    cache.insert_with_ttl(2, 20, Duration::from_secs(3600));

    // Inserting a third entry must evict 1 (soonest expiry).
    cache.insert_with_ttl(3, 30, Duration::from_secs(7200));
    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2), Some(20));
    assert_eq!(cache.get(&3), Some(30));
}

#[test]
fn ttl_eviction_prefers_already_expired_over_live() {
    let cache: TtlCache<u32, u32> = TtlCache::new(2, LONG_TTL).expect("capacity > 0");
    cache.insert_with_ttl(1, 10, SHORT_TTL); // will expire
    cache.insert(2, 20); // long-lived
    std::thread::sleep(EXPIRY_SLEEP);

    // 1 is expired-but-still-in-map; 2 is live. Adding 3 must drop 1, keep 2.
    cache.insert(3, 30);
    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2), Some(20));
    assert_eq!(cache.get(&3), Some(30));
}

#[test]
fn ttl_insert_returns_none_when_overwriting_expired_entry() {
    // Stale-but-still-in-map entry should be treated as absent on insert —
    // the user expects "expired = gone", not "expired = silently returned".
    let cache: TtlCache<u32, u32> = TtlCache::new(4, SHORT_TTL).expect("capacity > 0");
    cache.insert(1, 10);
    std::thread::sleep(EXPIRY_SLEEP);
    assert_eq!(cache.insert(1, 20), None);
    assert_eq!(cache.get(&1), Some(20));
}

#[test]
fn ttl_insert_returns_old_value_for_live_update() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, LONG_TTL).expect("capacity > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.insert(1, 20), Some(10));
    assert_eq!(cache.get(&1), Some(20));
}

#[test]
fn ttl_clear_empties_the_cache() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, LONG_TTL).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.capacity(), 4);
    assert_eq!(cache.get(&1), None);
}

#[test]
fn ttl_remove_returns_value() {
    let cache: TtlCache<u32, u32> = TtlCache::new(4, LONG_TTL).expect("capacity > 0");
    cache.insert(1, 10);
    assert_eq!(cache.remove(&1), Some(10));
    assert_eq!(cache.remove(&1), None);
    assert!(!cache.contains_key(&1));
}

#[test]
fn ttl_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<TtlCache<u32, String>>();
    assert_sync::<TtlCache<u32, String>>();
}

// -----------------------------------------------------------------------------
// TinyLfuCache (0.5.0)
// -----------------------------------------------------------------------------

#[test]
fn tinylfu_zero_capacity_is_rejected() {
    let err = TinyLfuCache::<u32, u32>::new(0).err();
    assert_eq!(err, Some(CacheError::InvalidCapacity));
}

#[test]
fn tinylfu_insert_then_get_returns_value() {
    let cache: TinyLfuCache<u32, u32> = TinyLfuCache::new(4).expect("capacity > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.len(), 1);
}

#[test]
fn tinylfu_existing_key_update_always_admits() {
    let cache: TinyLfuCache<u32, u32> = TinyLfuCache::new(2).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    // Updating an existing key bypasses the admission filter and returns
    // the previous value.
    assert_eq!(cache.insert(1, 11), Some(10));
    assert_eq!(cache.get(&1), Some(11));
}

#[test]
fn tinylfu_warm_candidate_wins_admission_over_cold_victim() {
    let cache: TinyLfuCache<u32, u32> = TinyLfuCache::new(2).expect("capacity > 0");

    // Fill the cache.
    cache.insert(1, 10);
    cache.insert(2, 20);
    // Build up substantial frequency for an outsider key (3) without
    // admitting it (admission filter rejects until 3 is "warmer" than
    // the coldest in-cache entry).
    for _ in 0..50 {
        // get(&3) bumps the sketch even on miss.
        let _ = cache.get(&3);
    }
    // Now try to admit 3 — its sketch frequency should exceed the
    // never-touched victims', so admission succeeds.
    cache.insert(3, 30);
    assert_eq!(cache.get(&3), Some(30));
}

#[test]
fn tinylfu_clear_resets_sketch() {
    let cache: TinyLfuCache<u32, u32> = TinyLfuCache::new(4).expect("capacity > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.capacity(), 4);
    assert_eq!(cache.get(&1), None);
}

#[test]
fn tinylfu_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<TinyLfuCache<u32, String>>();
    assert_sync::<TinyLfuCache<u32, String>>();
}

// -----------------------------------------------------------------------------
// SizedCache (0.5.0)
// -----------------------------------------------------------------------------

fn unit_weigher(_: &u32) -> usize {
    1
}

// The weigher's signature must exactly match `fn(&V) -> usize` for the
// `SizedCache<&'static str, Vec<u8>>` instances below, so `&Vec<u8>` is
// required here even though clippy would otherwise prefer `&[u8]`.
#[allow(clippy::ptr_arg)]
fn byte_weigher(v: &Vec<u8>) -> usize {
    v.len()
}

#[test]
fn sized_zero_max_weight_is_rejected() {
    let err = SizedCache::<u32, u32>::new(0, unit_weigher).err();
    assert_eq!(err, Some(CacheError::InvalidCapacity));
}

#[test]
fn sized_insert_then_get_returns_value() {
    let cache: SizedCache<u32, u32> = SizedCache::new(8, unit_weigher).expect("max_weight > 0");
    assert_eq!(cache.insert(1, 10), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.total_weight(), 1);
}

#[test]
fn sized_eviction_by_weight() {
    // max_weight = 3, each value weighs 1, so the cache holds up to 3 entries
    // and the 4th insert evicts the LRU.
    let cache: SizedCache<u32, u32> = SizedCache::new(3, unit_weigher).expect("max_weight > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    cache.insert(3, 30);
    // Access 1 -> 1 becomes MRU, 2 becomes LRU.
    let _ = cache.get(&1);
    cache.insert(4, 40); // evicts 2
    assert_eq!(cache.get(&2), None);
    assert_eq!(cache.get(&1), Some(10));
    assert_eq!(cache.get(&3), Some(30));
    assert_eq!(cache.get(&4), Some(40));
    assert_eq!(cache.total_weight(), 3);
}

#[test]
fn sized_byte_weight_tracked_accurately() {
    let cache: SizedCache<&'static str, Vec<u8>> =
        SizedCache::new(100, byte_weigher).expect("max_weight > 0");
    cache.insert("a", vec![0u8; 40]);
    cache.insert("b", vec![0u8; 30]);
    assert_eq!(cache.total_weight(), 70);

    // Inserting 50 more bytes (total 120) would overflow — must evict "a"
    // (the LRU) to make room. After eviction: 30 + 50 = 80 <= 100.
    cache.insert("c", vec![0u8; 50]);
    assert_eq!(cache.total_weight(), 80);
    assert!(!cache.contains_key(&"a"));
}

#[test]
fn sized_replace_updates_total_weight() {
    let cache: SizedCache<&'static str, Vec<u8>> =
        SizedCache::new(100, byte_weigher).expect("max_weight > 0");
    cache.insert("k", vec![0u8; 20]);
    assert_eq!(cache.total_weight(), 20);

    // Replacing with a larger value should bump total_weight.
    let old = cache.insert("k", vec![0u8; 60]);
    assert!(old.is_some());
    assert_eq!(cache.total_weight(), 60);
}

#[test]
fn sized_oversized_value_silently_rejected() {
    let cache: SizedCache<u32, Vec<u8>> =
        SizedCache::new(50, byte_weigher).expect("max_weight > 0");
    // Value larger than max_weight cannot fit. insert returns None and the
    // cache is unchanged.
    let result = cache.insert(1, vec![0u8; 100]);
    assert_eq!(result, None);
    assert!(!cache.contains_key(&1));
    assert_eq!(cache.total_weight(), 0);
}

#[test]
fn sized_remove_updates_total_weight() {
    let cache: SizedCache<&'static str, Vec<u8>> =
        SizedCache::new(100, byte_weigher).expect("max_weight > 0");
    cache.insert("a", vec![0u8; 30]);
    cache.insert("b", vec![0u8; 20]);
    assert_eq!(cache.total_weight(), 50);

    assert_eq!(cache.remove(&"a"), Some(vec![0u8; 30]));
    assert_eq!(cache.total_weight(), 20);
}

#[test]
fn sized_clear_resets_weight() {
    let cache: SizedCache<u32, u32> = SizedCache::new(8, unit_weigher).expect("max_weight > 0");
    cache.insert(1, 10);
    cache.insert(2, 20);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.total_weight(), 0);
    assert_eq!(cache.max_weight(), 8);
    assert_eq!(cache.capacity(), 8); // capacity() returns max_weight for SizedCache
}

#[test]
fn sized_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<SizedCache<u32, String>>();
    assert_sync::<SizedCache<u32, String>>();
}
