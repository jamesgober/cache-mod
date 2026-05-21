//! Property tests covering invariants every cache type must hold.
//!
//! These run the same sequence of operations against each cache type and
//! assert structural invariants: `len <= capacity` (or, for `SizedCache`,
//! `total_weight <= max_weight`), `clear` zeroes the cache, every value
//! stored matches the most recent `insert` for that key (within the cache's
//! eviction / admission contract), and the cache stays `Send + Sync`.

#![cfg(feature = "std")]

use std::time::Duration;

use cache_mod::{Cache, LfuCache, LruCache, SizedCache, TinyLfuCache, TtlCache};
use proptest::collection::vec;
use proptest::prelude::*;

/// A scripted operation applied during property tests. Keys are `u8` to
/// force collisions and increase the chance of hitting eviction paths.
#[derive(Debug, Clone)]
enum Op {
    Insert(u8, u8),
    Get(u8),
    Remove(u8),
    ContainsKey(u8),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<u8>(), any::<u8>()).prop_map(|(k, v)| Op::Insert(k, v)),
        any::<u8>().prop_map(Op::Get),
        any::<u8>().prop_map(Op::Remove),
        any::<u8>().prop_map(Op::ContainsKey),
    ]
}

fn ops_strategy() -> impl Strategy<Value = Vec<Op>> {
    vec(op_strategy(), 0..200)
}

/// Apply a scripted op sequence to any `Cache` and assert capacity invariant.
fn drive<C>(cache: &C, ops: &[Op], capacity_bound: usize)
where
    C: Cache<u8, u8>,
{
    for op in ops {
        match op {
            Op::Insert(k, v) => {
                let _ = cache.insert(*k, *v);
            }
            Op::Get(k) => {
                let _ = cache.get(k);
            }
            Op::Remove(k) => {
                let _ = cache.remove(k);
            }
            Op::ContainsKey(k) => {
                let _ = cache.contains_key(k);
            }
        }
        assert!(
            cache.len() <= capacity_bound,
            "len {} exceeded capacity bound {}",
            cache.len(),
            capacity_bound,
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn lru_never_exceeds_capacity(ops in ops_strategy()) {
        let cache: LruCache<u8, u8> = LruCache::new(8).expect("capacity > 0");
        drive(&cache, &ops, 8);
        cache.clear();
        prop_assert_eq!(cache.len(), 0);
        prop_assert!(cache.is_empty());
    }

    #[test]
    fn lfu_never_exceeds_capacity(ops in ops_strategy()) {
        let cache: LfuCache<u8, u8> = LfuCache::new(8).expect("capacity > 0");
        drive(&cache, &ops, 8);
        cache.clear();
        prop_assert_eq!(cache.len(), 0);
    }

    #[test]
    fn ttl_never_exceeds_capacity(ops in ops_strategy()) {
        let cache: TtlCache<u8, u8> = TtlCache::new(8, Duration::from_secs(60)).expect("capacity > 0");
        drive(&cache, &ops, 8);
        cache.clear();
        prop_assert_eq!(cache.len(), 0);
    }

    #[test]
    fn tinylfu_never_exceeds_capacity(ops in ops_strategy()) {
        let cache: TinyLfuCache<u8, u8> = TinyLfuCache::new(8).expect("capacity > 0");
        drive(&cache, &ops, 8);
        cache.clear();
        prop_assert_eq!(cache.len(), 0);
    }

    #[test]
    fn sized_never_exceeds_max_weight(ops in ops_strategy()) {
        // Each value contributes 1 to the weight — so max_weight = 8 caps
        // the entry count at 8, the same bound as the other caches.
        fn unit_weight(_: &u8) -> usize { 1 }
        let cache: SizedCache<u8, u8> = SizedCache::new(8, unit_weight).expect("max_weight > 0");
        for op in &ops {
            match op {
                Op::Insert(k, v) => { let _ = cache.insert(*k, *v); }
                Op::Get(k) => { let _ = cache.get(k); }
                Op::Remove(k) => { let _ = cache.remove(k); }
                Op::ContainsKey(k) => { let _ = cache.contains_key(k); }
            }
            prop_assert!(cache.total_weight() <= cache.max_weight());
            prop_assert!(cache.len() <= cache.max_weight());
        }
        cache.clear();
        prop_assert_eq!(cache.total_weight(), 0);
        prop_assert_eq!(cache.len(), 0);
    }
}

// Insert-then-get must return the inserted value, *unless* the cache rejects
// the admission (only `TinyLfuCache`) or evicts the entry under a later op.
// This property is scoped to "insert immediately followed by get" — no other
// ops between — so eviction can't intervene.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn lru_insert_then_get_round_trips(k in any::<u8>(), v in any::<u8>()) {
        let cache: LruCache<u8, u8> = LruCache::new(16).expect("capacity > 0");
        let _ = cache.insert(k, v);
        prop_assert_eq!(cache.get(&k), Some(v));
    }

    #[test]
    fn lfu_insert_then_get_round_trips(k in any::<u8>(), v in any::<u8>()) {
        let cache: LfuCache<u8, u8> = LfuCache::new(16).expect("capacity > 0");
        let _ = cache.insert(k, v);
        prop_assert_eq!(cache.get(&k), Some(v));
    }

    #[test]
    fn ttl_insert_then_get_round_trips(k in any::<u8>(), v in any::<u8>()) {
        let cache: TtlCache<u8, u8> = TtlCache::new(16, Duration::from_secs(60)).expect("capacity > 0");
        let _ = cache.insert(k, v);
        prop_assert_eq!(cache.get(&k), Some(v));
    }

    #[test]
    fn sized_insert_then_get_round_trips(k in any::<u8>(), v in any::<u8>()) {
        fn unit_weight(_: &u8) -> usize { 1 }
        let cache: SizedCache<u8, u8> = SizedCache::new(16, unit_weight).expect("max_weight > 0");
        let _ = cache.insert(k, v);
        prop_assert_eq!(cache.get(&k), Some(v));
    }
}

// -----------------------------------------------------------------------------
// Concurrent-safety properties.
//
// Each test spawns N threads driving random operations against a single
// shared cache. The asserted property is conservative — the cache must
// never observably violate its capacity invariant or panic — regardless
// of interleaving. We deliberately do NOT assert specific values or hit
// rates, since concurrent interleaving makes those non-deterministic.
// -----------------------------------------------------------------------------

use std::sync::Arc;
use std::thread;

const CONCURRENT_THREADS: usize = 4;
const CONCURRENT_OPS_PER_THREAD: usize = 256;

fn run_concurrent_ops<C>(cache: Arc<C>, ops: Vec<Op>, capacity_bound: usize)
where
    C: Cache<u8, u8> + Send + Sync + 'static,
{
    // Partition the op stream across threads — each thread gets a roughly
    // equal slice and applies them serially. Cross-thread interleaving
    // emerges from the OS scheduler.
    let chunk = ops.len() / CONCURRENT_THREADS.max(1);
    let chunks: Vec<Vec<Op>> = (0..CONCURRENT_THREADS)
        .map(|t| {
            let start = t * chunk;
            let end = if t + 1 == CONCURRENT_THREADS {
                ops.len()
            } else {
                start + chunk
            };
            ops[start..end].to_vec()
        })
        .collect();

    let handles: Vec<_> = chunks
        .into_iter()
        .map(|chunk| {
            let cache = Arc::clone(&cache);
            thread::spawn(move || {
                for op in chunk {
                    match op {
                        Op::Insert(k, v) => {
                            let _ = cache.insert(k, v);
                        }
                        Op::Get(k) => {
                            let _ = cache.get(&k);
                        }
                        Op::Remove(k) => {
                            let _ = cache.remove(&k);
                        }
                        Op::ContainsKey(k) => {
                            let _ = cache.contains_key(&k);
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("worker thread should not panic");
    }

    // After every thread has joined, the cache must still respect its
    // capacity bound.
    assert!(
        cache.len() <= capacity_bound,
        "post-join len {} exceeded capacity bound {}",
        cache.len(),
        capacity_bound,
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn lru_concurrent_threads_preserve_capacity(
        ops in vec(op_strategy(), CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD..=CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD + 1),
    ) {
        // Capacity 64 → 4 shards × 16 each. Exercises sharded code path.
        let cache: Arc<LruCache<u8, u8>> = Arc::new(LruCache::new(64).expect("capacity > 0"));
        run_concurrent_ops(cache, ops, 64);
    }

    #[test]
    fn lfu_concurrent_threads_preserve_capacity(
        ops in vec(op_strategy(), CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD..=CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD + 1),
    ) {
        let cache: Arc<LfuCache<u8, u8>> = Arc::new(LfuCache::new(64).expect("capacity > 0"));
        run_concurrent_ops(cache, ops, 64);
    }

    #[test]
    fn ttl_concurrent_threads_preserve_capacity(
        ops in vec(op_strategy(), CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD..=CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD + 1),
    ) {
        let cache: Arc<TtlCache<u8, u8>> =
            Arc::new(TtlCache::new(64, Duration::from_secs(60)).expect("capacity > 0"));
        run_concurrent_ops(cache, ops, 64);
    }

    #[test]
    fn tinylfu_concurrent_threads_preserve_capacity(
        ops in vec(op_strategy(), CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD..=CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD + 1),
    ) {
        let cache: Arc<TinyLfuCache<u8, u8>> = Arc::new(TinyLfuCache::new(64).expect("capacity > 0"));
        run_concurrent_ops(cache, ops, 64);
    }
}

// -----------------------------------------------------------------------------
// Cross-cache symmetry properties — invariants that should hold regardless
// of which `Cache` implementation is under test.
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn lru_remove_then_contains_key_returns_false(k in any::<u8>(), v in any::<u8>()) {
        let cache: LruCache<u8, u8> = LruCache::new(16).expect("capacity > 0");
        let _ = cache.insert(k, v);
        prop_assert!(cache.contains_key(&k));
        let _ = cache.remove(&k);
        prop_assert!(!cache.contains_key(&k));
        prop_assert_eq!(cache.get(&k), None);
    }

    #[test]
    fn lfu_remove_then_contains_key_returns_false(k in any::<u8>(), v in any::<u8>()) {
        let cache: LfuCache<u8, u8> = LfuCache::new(16).expect("capacity > 0");
        let _ = cache.insert(k, v);
        prop_assert!(cache.contains_key(&k));
        let _ = cache.remove(&k);
        prop_assert!(!cache.contains_key(&k));
        prop_assert_eq!(cache.get(&k), None);
    }

    #[test]
    fn ttl_remove_then_contains_key_returns_false(k in any::<u8>(), v in any::<u8>()) {
        let cache: TtlCache<u8, u8> =
            TtlCache::new(16, Duration::from_secs(60)).expect("capacity > 0");
        let _ = cache.insert(k, v);
        prop_assert!(cache.contains_key(&k));
        let _ = cache.remove(&k);
        prop_assert!(!cache.contains_key(&k));
        prop_assert_eq!(cache.get(&k), None);
    }

    #[test]
    fn sized_remove_then_contains_key_returns_false(k in any::<u8>(), v in any::<u8>()) {
        fn unit_weight(_: &u8) -> usize { 1 }
        let cache: SizedCache<u8, u8> = SizedCache::new(16, unit_weight).expect("max_weight > 0");
        let _ = cache.insert(k, v);
        prop_assert!(cache.contains_key(&k));
        let _ = cache.remove(&k);
        prop_assert!(!cache.contains_key(&k));
        prop_assert_eq!(cache.get(&k), None);
    }
}
