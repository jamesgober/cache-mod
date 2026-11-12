//! Regression tests for defects fixed after 1.0.0.

#![cfg(feature = "std")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cache_mod::{Cache, LfuCache, LruCache, SizedCache, TinyLfuCache, TtlCache};
use proptest::collection::vec;
use proptest::prelude::*;

#[allow(clippy::ptr_arg)] // the weigher signature is fn(&V)
fn weigh(s: &String) -> usize {
    s.len()
}

// A growing update that pushes the total over budget evicts other entries.
// Before 1.1.0 their index entries stayed behind, so a later lookup
// panicked or returned another key's value.
#[test]
fn sized_growing_update_removes_evicted_keys() {
    let c: SizedCache<&'static str, String> = SizedCache::new(100, weigh).unwrap();
    let _ = c.insert("alice", "A".repeat(40));
    let _ = c.insert("bob", "B".repeat(30));
    let _ = c.insert("bob", "B".repeat(80));
    assert_eq!(c.total_weight(), 80);
    assert_eq!(c.len(), 1);
    assert!(!c.contains_key(&"alice"));

    // The freed slot is reused here; alice must not see carol's value.
    let _ = c.insert("carol", "C".repeat(10));
    assert_eq!(c.get(&"alice"), None);
    assert_eq!(c.remove(&"alice"), None);
    assert_eq!(c.get(&"carol").as_deref(), Some("CCCCCCCCCC"));
    assert_eq!(c.len(), 2);
}

#[test]
fn sized_growing_update_then_get_does_not_panic() {
    let c: SizedCache<&'static str, String> = SizedCache::new(100, weigh).unwrap();
    let _ = c.insert("alice", "A".repeat(40));
    let _ = c.insert("bob", "B".repeat(30));
    let _ = c.insert("bob", "B".repeat(80));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.get(&"alice")));
    assert_eq!(r.ok(), Some(None));
}

// Model of SizedCache: a recency list, most recent first.
struct Model {
    max: usize,
    order: VecDeque<(u8, String)>,
}

impl Model {
    fn total(&self) -> usize {
        self.order.iter().map(|(_, v)| v.len()).sum()
    }

    fn pos(&self, k: u8) -> Option<usize> {
        self.order.iter().position(|(key, _)| *key == k)
    }

    fn insert(&mut self, k: u8, v: String) -> Option<String> {
        if v.len() > self.max {
            return None;
        }
        if let Some(i) = self.pos(k) {
            let (_, old) = self.order.remove(i).unwrap();
            self.order.push_front((k, v));
            while self.total() > self.max {
                let _ = self.order.pop_back();
            }
            return Some(old);
        }
        while self.total() + v.len() > self.max {
            if self.order.pop_back().is_none() {
                break;
            }
        }
        self.order.push_front((k, v));
        None
    }

    fn get(&mut self, k: u8) -> Option<String> {
        let i = self.pos(k)?;
        let entry = self.order.remove(i).unwrap();
        let v = entry.1.clone();
        self.order.push_front(entry);
        Some(v)
    }

    fn remove(&mut self, k: u8) -> Option<String> {
        let i = self.pos(k)?;
        self.order.remove(i).map(|(_, v)| v)
    }
}

#[derive(Debug, Clone)]
enum SizedOp {
    Insert(u8, usize),
    Get(u8),
    Remove(u8),
}

fn sized_op() -> impl Strategy<Value = SizedOp> {
    prop_oneof![
        5 => (0u8..12, 0usize..90).prop_map(|(k, w)| SizedOp::Insert(k, w)),
        3 => (0u8..12).prop_map(SizedOp::Get),
        1 => (0u8..12).prop_map(SizedOp::Remove),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // Values grow and shrink on update; the cache must match the model on
    // every return value and on every key after every step.
    #[test]
    fn sized_matches_model_with_resizing_values(ops in vec(sized_op(), 1..300)) {
        let max = 100;
        let c: SizedCache<u8, String> = SizedCache::new(max, weigh).unwrap();
        let mut m = Model { max, order: VecDeque::new() };
        for (step, op) in ops.into_iter().enumerate() {
            match op {
                SizedOp::Insert(k, w) => {
                    // Tag the value with the key and step so cross-wiring shows.
                    let v: String = format!("{k}:{step}:").chars().chain(std::iter::repeat('x')).take(w).collect();
                    prop_assert_eq!(c.insert(k, v.clone()), m.insert(k, v));
                }
                SizedOp::Get(k) => prop_assert_eq!(c.get(&k), m.get(k)),
                SizedOp::Remove(k) => prop_assert_eq!(c.remove(&k), m.remove(k)),
            }
            prop_assert_eq!(c.len(), m.order.len());
            prop_assert_eq!(c.total_weight(), m.total());
            prop_assert!(c.total_weight() <= max);
            for k in 0u8..12 {
                prop_assert_eq!(c.contains_key(&k), m.pos(k).is_some(), "key {}", k);
            }
        }
    }
}

// A key whose `Eq` can be made to panic.
static PANIC_EQ: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
struct Touchy(u32);

impl PartialEq for Touchy {
    fn eq(&self, o: &Self) -> bool {
        if PANIC_EQ.load(Ordering::SeqCst) {
            panic!("eq");
        }
        self.0 == o.0
    }
}
impl Eq for Touchy {}
impl Hash for Touchy {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}

// Before 1.1.0 the evicted slot was freed before the index entry was
// removed, so a panic in `Eq` left the index pointing at a freed slot.
#[test]
fn lru_panicking_eq_during_eviction_keeps_index_consistent() {
    let c: LruCache<Touchy, u32> = LruCache::new(2).unwrap();
    let _ = c.insert(Touchy(1), 100);
    let _ = c.insert(Touchy(2), 200);
    PANIC_EQ.store(true, Ordering::SeqCst);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.insert(Touchy(3), 300)));
    PANIC_EQ.store(false, Ordering::SeqCst);
    assert!(r.is_err());
    for _ in 0..3 {
        let g = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.get(&Touchy(1))));
        assert_eq!(
            g.ok(),
            Some(Some(100)),
            "get after the panic must not panic"
        );
    }
    assert_eq!(c.get(&Touchy(2)), Some(200));
    let _ = c.insert(Touchy(4), 400);
    assert_eq!(c.len(), 2);
}

// Before 1.1.0 the constructors reserved the full capacity up front and
// panicked with "capacity overflow" on huge values.
#[test]
fn huge_capacities_do_not_panic_in_constructors() {
    let lru = LruCache::<u64, u64>::new(usize::MAX).unwrap();
    let _ = lru.insert(1, 1);
    assert_eq!(lru.get(&1), Some(1));
    let lfu = LfuCache::<u64, u64>::new(usize::MAX).unwrap();
    let _ = lfu.insert(1, 1);
    assert_eq!(lfu.get(&1), Some(1));
    let ttl = TtlCache::<u64, u64>::new(usize::MAX, Duration::from_secs(60)).unwrap();
    let _ = ttl.insert(1, 1);
    assert_eq!(ttl.get(&1), Some(1));
    let tiny = TinyLfuCache::<u64, u64>::new(usize::MAX / 2).unwrap();
    let _ = tiny.insert(1, 1);
    assert_eq!(tiny.get(&1), Some(1));
}

// TtlCache picks the soonest-expiring victim from a deadline index; the
// eviction order must be the same as the old full scan.
#[test]
fn ttl_overflow_still_evicts_soonest_deadline_when_sharded() {
    let c: TtlCache<u32, u32> = TtlCache::new(1024, Duration::from_secs(10_000)).unwrap();
    for i in 0..1024u32 {
        let _ = c.insert_with_ttl(i, i, Duration::from_secs(60 + u64::from(i)));
    }
    for i in 0..64u32 {
        let _ = c.insert(10_000 + i, i);
    }
    assert!(c.len() <= c.capacity());
    // The newest inserts hold the latest deadlines and must all survive.
    for i in 0..64u32 {
        assert!(c.contains_key(&(10_000 + i)));
    }
}
