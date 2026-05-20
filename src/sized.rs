//! Byte-bound cache — arena-backed reference implementation.

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
/// 0.6.0 implementation: arena-backed doubly-linked list with O(1) promote
/// and O(1) eviction (eviction may loop until enough weight is reclaimed).
/// The lock-strategy upgrade lands in 0.7.0 without changing this public surface.
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

    /// Pop the LRU tail and return the evicted node's (key, weight).
    fn evict_tail(&mut self) -> Option<(K, usize)> {
        let tail_idx = self.tail?;
        self.unlink(tail_idx);
        let node = self.dealloc(tail_idx);
        self.total_weight = self.total_weight.saturating_sub(node.weight);
        Some((node.key, node.weight))
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
}

impl<K, V> Cache<K, V> for SizedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let idx = *inner.map.get(key)?;
        inner.promote(idx);
        inner.nodes[idx].as_ref().map(|n| n.value.clone())
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
            // Live update may have overshot — evict from LRU until fit.
            while inner.total_weight > self.max_weight {
                if inner.evict_tail().is_none() {
                    break;
                }
            }
            // Pop the updated key from any map entry left dangling if it
            // was evicted. (Shouldn't happen — we promoted it to head — but
            // be defensive.)
            return Some(old_value);
        }

        // New entry — evict from the LRU end until the projected total fits.
        while inner.total_weight.saturating_add(new_weight) > self.max_weight {
            match inner.evict_tail() {
                Some((evicted_key, _)) => {
                    let _ = inner.map.remove(&evicted_key);
                }
                None => break,
            }
        }

        let idx = inner.alloc(Node {
            key: key.clone(),
            value,
            weight: new_weight,
            prev: None,
            next: None,
        });
        inner.push_front(idx);
        let _ = inner.map.insert(key, idx);
        inner.total_weight = inner.total_weight.saturating_add(new_weight);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let idx = inner.map.remove(key)?;
        inner.unlink(idx);
        let node = inner.dealloc(idx);
        inner.total_weight = inner.total_weight.saturating_sub(node.weight);
        Some(node.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.inner.lock_recover().map.contains_key(key)
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
