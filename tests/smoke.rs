//! End-to-end smoke tests for the 0.2.0 public surface.
//!
//! These verify that `Cache`, `LruCache`, and `CacheError` are wired up
//! correctly and behave per their documented contracts.
//!
//! Gated on the `std` feature because `LruCache` is `std`-only in 0.2.0.

#![cfg(feature = "std")]

use cache_mod::{Cache, CacheError, LruCache};

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
