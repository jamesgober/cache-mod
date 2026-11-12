//! Byte-bound cache — arena-backed, single-`Mutex` implementation.

use core::borrow::Borrow;
use core::hash::Hash;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::util::MutexExt;

/// A cache bounded by **total byte-weight** rather than entry count.
///
/// Each value is weighed by a user-supplied `fn(&V) -> usize` at insert
/// time. The cache evicts least-recently-accessed entries until the new
/// entry fits within `max_weight`.
///
/// An entry whose own weight exceeds `max_weight` is silently rejected:
/// the insert returns `None` and the value is dropped. (No cache could
/// honour such a request.)
///
/// # Implementation
///
/// Arena-backed doubly-linked list with O(1) promote and O(1) eviction
/// (eviction may loop until enough weight is reclaimed). Unlike the other
/// cache types in this crate, `SizedCache` is not sharded — splitting the
/// weight budget across shards would silently reject values that fit the
/// global budget, which is the wrong contract for a byte-bound cache.
/// Internal layout may evolve in future minor releases without changing
/// this public surface.
///
/// # Choice of unit
///
/// `usize` is unitless — the weigher decides. Common choices:
///
/// - `|v: &Vec<u8>| v.len()` — track payload bytes
/// - `|v: &String| v.len() + std::mem::size_of::<String>()` — include header
/// - `|_: &T| std::mem::size_of::<T>()` — fixed-size approximation
///
/// The weigher is a plain function pointer, not a closure — captured
/// state would force `Box<dyn Fn>`-style indirection on every weigh call.
/// If your weighing logic needs state, hoist it into the value itself.
///
/// # What the budget covers
///
/// The bound is the sum of the weigher's numbers and nothing else. It is
/// not a memory limit:
///
/// - Keys are not weighed. The weigher only sees `&V`, so key bytes, the
///   second copy of each key held by the index, and the per-entry node and
///   hash-table overhead (several tens of bytes per entry) are outside the
///   budget.
/// - A value the weigher scores as `0` costs nothing against the budget,
///   so a stream of zero-weight values (for example empty strings weighed
///   with `String::len`) grows the entry count without limit.
/// - The internal arena keeps its high-water size until
///   [`clear`](Cache::clear).
///
/// If the cache holds data from untrusted sources, make the weigher
/// return at least a fixed per-entry charge, for example `v.len() + 64`.
/// Every entry then costs something, and the entry count is bounded by
/// `max_weight / 64`.
///
/// # Capacity reporting
///
/// [`capacity`](Cache::capacity) on `SizedCache` returns `max_weight`, not
/// entry count. Use [`total_weight`](SizedCache::total_weight) to query
/// the dynamic in-use weight and [`max_weight`](SizedCache::max_weight)
/// as the explicit alias.
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, SizedCache};
///
/// fn weigh(payload: &Vec<u8>) -> usize { payload.len() }
///
/// let cache: SizedCache<&'static str, Vec<u8>> =
///     SizedCache::new(1024, weigh).expect("max_weight > 0");
///
/// cache.insert("small", vec![0u8; 64]);
/// assert_eq!(cache.total_weight(), 64);
/// ```
pub struct SizedCache<K, V> {
    max_weight: usize,
    weigher: fn(&V) -> usize,
    inner: Mutex<Inner<K, V>>,
}

struct Node<K, V> {
    key: K,
    value: V,
    weight: usize,
    prev: Option<usize>,
    next: Option<usize>,
}

struct Inner<K, V> {
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
    map: HashMap<K, usize>,
    total_weight: usize,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            free: Vec::new(),
            head: None,
            tail: None,
            map: HashMap::new(),
            total_weight: 0,
        }
    }

    fn alloc(&mut self, node: Node<K, V>) -> usize {
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    fn dealloc(&mut self, idx: usize) -> Node<K, V> {
        let node = self.nodes[idx]
            .take()
            .unwrap_or_else(|| unreachable!("arena slot must be occupied"));
        self.free.push(idx);
        node
    }

    fn unlink(&mut self, idx: usize) {
        let (prev, next) = {
            let n = self.nodes[idx]
                .as_ref()
                .unwrap_or_else(|| unreachable!("unlink target must be occupied"));
            (n.prev, n.next)
        };
        match prev {
            Some(p) => {
                self.nodes[p]
                    .as_mut()
                    .unwrap_or_else(|| unreachable!())
                    .next = next
            }
            None => self.head = next,
        }
        match next {
            Some(n) => {
                self.nodes[n]
                    .as_mut()
                    .unwrap_or_else(|| unreachable!())
                    .prev = prev
            }
            None => self.tail = prev,
        }
        if let Some(n) = self.nodes[idx].as_mut() {
            n.prev = None;
            n.next = None;
        }
    }

    fn push_front(&mut self, idx: usize) {
        let old_head = self.head;
        if let Some(n) = self.nodes[idx].as_mut() {
            n.prev = None;
            n.next = old_head;
        }
        if let Some(h) = old_head {
            if let Some(n) = self.nodes[h].as_mut() {
                n.prev = Some(idx);
            }
        } else {
            self.tail = Some(idx);
        }
        self.head = Some(idx);
    }

    fn promote(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return;
        }
        self.unlink(idx);
        self.push_front(idx);
    }

    /// Evict the LRU tail: drop its index entry, unlink it, free its slot
    /// and subtract its weight. Returns `false` when the list is empty.
    ///
    /// Every eviction path goes through here, so the index and the arena
    /// cannot drift apart. The index entry is removed first: if the
    /// user's `Hash` or `Eq` panics during that removal, nothing has
    /// changed yet.
    fn evict_tail(&mut self) -> bool {
        let Some(tail_idx) = self.tail else {
            return false;
        };
        if let Some(node) = self.nodes[tail_idx].as_ref() {
            let _ = self.map.remove(&node.key);
        }
        let _ = self.take(tail_idx);
        true
    }

    /// Unlink and free the node at `idx`, which the caller has already
    /// removed from the index, and return its value.
    fn take(&mut self, idx: usize) -> V {
        self.unlink(idx);
        let node = self.dealloc(idx);
        self.total_weight = self.total_weight.saturating_sub(node.weight);
        node.value
    }

    /// Assert the internal invariants. Test builds only.
    #[cfg(test)]
    fn check_invariants(&self) {
        let live = self.nodes.iter().filter(|n| n.is_some()).count();
        assert_eq!(self.map.len(), live, "index size must equal live nodes");
        assert_eq!(
            self.nodes.len() - live,
            self.free.len(),
            "every empty slot is on the free list"
        );
        let mut weight = 0usize;
        let mut walked = 0usize;
        let mut cur = self.head;
        let mut prev = None;
        while let Some(idx) = cur {
            let node = match self.nodes[idx].as_ref() {
                Some(n) => n,
                None => panic!("listed slot {idx} is empty"),
            };
            assert_eq!(node.prev, prev, "prev link of slot {idx}");
            assert_eq!(self.map.get(&node.key), Some(&idx), "index -> slot {idx}");
            weight += node.weight;
            walked += 1;
            prev = Some(idx);
            cur = node.next;
        }
        assert_eq!(self.tail, prev, "tail is the last listed node");
        assert_eq!(walked, live, "every live node is listed");
        assert_eq!(weight, self.total_weight, "total weight");
    }
}

impl<K, V> SizedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a cache with the given byte-weight ceiling.
    ///
    /// Returns [`CacheError::InvalidCapacity`] if `max_weight == 0`.
    ///
    /// # Example
    ///
    /// ```
    /// use cache_mod::SizedCache;
    ///
    /// fn weigh(s: &String) -> usize { s.len() }
    ///
    /// let cache: SizedCache<u32, String> =
    ///     SizedCache::new(4096, weigh).expect("max_weight > 0");
    /// ```
    pub fn new(max_weight: usize, weigher: fn(&V) -> usize) -> Result<Self, CacheError> {
        if max_weight == 0 {
            return Err(CacheError::InvalidCapacity);
        }
        Ok(Self {
            max_weight,
            weigher,
            inner: Mutex::new(Inner::new()),
        })
    }

    /// The configured byte-weight ceiling.
    pub fn max_weight(&self) -> usize {
        self.max_weight
    }

    /// Current total weight of all cached entries.
    pub fn total_weight(&self) -> usize {
        self.inner.lock_recover().total_weight
    }

    /// Like [`get`](Cache::get), but takes any borrowed form of the key,
    /// so a `SizedCache<String, V>` can be queried with a `&str` without
    /// building a `String`.
    ///
    /// # Example
    ///
    /// ```
    /// use cache_mod::{Cache, SizedCache};
    ///
    /// fn weigh(s: &String) -> usize { s.len() }
    ///
    /// let cache: SizedCache<String, String> =
    ///     SizedCache::new(1024, weigh).expect("max_weight > 0");
    /// cache.insert("user:1".to_string(), "alice".to_string());
    /// assert_eq!(cache.get_by("user:1").as_deref(), Some("alice"));
    /// ```
    pub fn get_by<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.inner.lock_recover();
        let idx = *inner.map.get(key)?;
        inner.promote(idx);
        inner.nodes[idx].as_ref().map(|n| n.value.clone())
    }

    /// Like [`contains_key`](Cache::contains_key), but takes any borrowed
    /// form of the key.
    pub fn contains_key_by<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.inner.lock_recover().map.contains_key(key)
    }

    /// Like [`remove`](Cache::remove), but takes any borrowed form of the
    /// key.
    pub fn remove_by<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut inner = self.inner.lock_recover();
        let idx = inner.map.remove(key)?;
        Some(inner.take(idx))
    }
}

impl<K, V> Cache<K, V> for SizedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        self.get_by(key)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let new_weight = (self.weigher)(&value);
        // Reject values that cannot possibly fit.
        if new_weight > self.max_weight {
            return None;
        }

        let mut inner = self.inner.lock_recover();

        // Live update: replace value, fix up weight delta, promote.
        if let Some(&idx) = inner.map.get(&key) {
            let (old_value, old_weight) = inner.nodes[idx]
                .as_mut()
                .map(|n| {
                    let ov = core::mem::replace(&mut n.value, value);
                    let ow = n.weight;
                    n.weight = new_weight;
                    (ov, ow)
                })
                .unwrap_or_else(|| unreachable!("mapped index must be occupied"));
            inner.total_weight = inner
                .total_weight
                .saturating_add(new_weight)
                .saturating_sub(old_weight);
            inner.promote(idx);
            // A growing update can overshoot the budget: evict from the
            // LRU end until it fits. The updated entry is now the head and
            // its own weight fits, so it is never the one evicted.
            while inner.total_weight > self.max_weight && inner.evict_tail() {}
            return Some(old_value);
        }

        // New entry: evict from the LRU end until the projected total fits.
        while inner.total_weight.saturating_add(new_weight) > self.max_weight && inner.evict_tail()
        {
        }

        let idx = inner.alloc(Node {
            key: key.clone(),
            value,
            weight: new_weight,
            prev: None,
            next: None,
        });
        // Index first, then link: if the user's `Hash` panics here the
        // node is unreachable but the list and the index still agree.
        let _ = inner.map.insert(key, idx);
        inner.push_front(idx);
        inner.total_weight = inner.total_weight.saturating_add(new_weight);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        self.remove_by(key)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.contains_key_by(key)
    }

    fn len(&self) -> usize {
        self.inner.lock_recover().map.len()
    }

    fn clear(&self) {
        let mut inner = self.inner.lock_recover();
        inner.nodes.clear();
        inner.free.clear();
        inner.head = None;
        inner.tail = None;
        inner.map.clear();
        inner.total_weight = 0;
    }

    /// For `SizedCache`, capacity is the configured `max_weight` — see the
    /// type-level docs.
    fn capacity(&self) -> usize {
        self.max_weight
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    #[allow(clippy::ptr_arg)] // the weigher signature is fn(&V)
    fn weigh(v: &Vec<u8>) -> usize {
        v.len()
    }

    impl<K: Eq + Hash + Clone, V> SizedCache<K, V> {
        fn check(&self) {
            self.inner.lock_recover().check_invariants();
        }
    }

    #[test]
    fn growing_update_evicts_and_drops_index_entries() {
        let c: SizedCache<&'static str, Vec<u8>> = SizedCache::new(100, weigh).unwrap();
        let _ = c.insert("alice", vec![b'a'; 40]);
        let _ = c.insert("bob", vec![b'b'; 30]);
        let _ = c.insert("bob", vec![b'b'; 80]);
        c.check();
        assert_eq!(c.len(), 1);
        assert!(!c.contains_key(&"alice"));
        assert_eq!(c.get(&"alice"), None);
        let _ = c.insert("carol", vec![b'c'; 10]);
        c.check();
        assert_eq!(c.get(&"alice"), None);
        assert_eq!(c.get(&"carol"), Some(vec![b'c'; 10]));
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u8, usize),
        Get(u8),
        Remove(u8),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => (0u8..10, 0usize..70).prop_map(|(k, w)| Op::Insert(k, w)),
            2 => (0u8..10).prop_map(Op::Get),
            1 => (0u8..10).prop_map(Op::Remove),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        // Random grows and shrinks keep the index, the list and the weight
        // in step after every operation.
        #[test]
        fn invariants_hold_under_resizing_updates(ops in vec(op(), 1..200)) {
            let c: SizedCache<u8, Vec<u8>> = SizedCache::new(100, weigh).unwrap();
            for op in ops {
                match op {
                    Op::Insert(k, w) => { let _ = c.insert(k, vec![k; w]); }
                    Op::Get(k) => {
                        if let Some(v) = c.get(&k) {
                            prop_assert!(v.iter().all(|b| *b == k), "value of another key");
                        }
                    }
                    Op::Remove(k) => { let _ = c.remove(&k); }
                }
                c.check();
                prop_assert!(c.total_weight() <= c.max_weight());
            }
        }
    }
}
