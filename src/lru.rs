//! Least-Recently-Used (LRU) cache — sharded, arena-backed implementation.

use core::hash::Hash;
use std::collections::HashMap;
use std::num::NonZeroUsize;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::sharding::{self, Sharded};
use crate::util::MutexExt;

/// A bounded, thread-safe LRU cache.
///
/// On insert overflow the least-recently-accessed entry is evicted. Both
/// [`get`](Cache::get) and [`insert`](Cache::insert) count as accesses and
/// promote the affected entry to most-recently-used.
///
/// # Implementation (0.7.0)
///
/// Sharded into up to 16 independent arenas keyed by hash of `K`. Each shard
/// owns its own doubly-linked list, free-list, and `HashMap`, with its own
/// `Mutex<Inner>`. Contention on the lock is bounded by the number of
/// threads routing into the same shard, not by total cache traffic.
///
/// **Eviction is approximate.** Once the cache uses more than one shard,
/// `insert` overflow evicts the local-to-shard least-recently-used entry,
/// not the global one. Caches with fewer than 32 entries automatically use
/// a single shard and retain strict global LRU ordering — this keeps small
/// caches and test fixtures deterministic.
///
/// # Example
///
/// ```
/// use cache_mod::{Cache, LruCache};
///
/// let cache: LruCache<u32, &'static str> = LruCache::new(2).expect("capacity > 0");
///
/// cache.insert(1, "one");
/// cache.insert(2, "two");
/// assert_eq!(cache.get(&1), Some("one")); // 1 is now MRU, 2 is LRU
///
/// cache.insert(3, "three"); // evicts 2 (single shard at this size)
/// assert_eq!(cache.get(&2), None);
/// assert_eq!(cache.get(&1), Some("one"));
/// assert_eq!(cache.get(&3), Some("three"));
/// ```
pub struct LruCache<K, V> {
    capacity: NonZeroUsize,
    sharded: Sharded<Inner<K, V>>,
}

struct Node<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

struct Inner<K, V> {
    /// Per-shard capacity (not the total LruCache capacity).
    capacity: NonZeroUsize,
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
    map: HashMap<K, usize>,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn with_capacity(capacity: NonZeroUsize) -> Self {
        let cap = capacity.get();
        Self {
            capacity,
            nodes: Vec::with_capacity(cap),
            free: Vec::new(),
            head: None,
            tail: None,
            map: HashMap::with_capacity(cap),
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
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a cache with the given capacity.
    ///
    /// Returns [`CacheError::InvalidCapacity`] if `capacity == 0`.
    ///
    /// # Example
    ///
    /// ```
    /// use cache_mod::LruCache;
    ///
    /// let cache: LruCache<String, u32> = LruCache::new(128).expect("capacity > 0");
    /// ```
    pub fn new(capacity: usize) -> Result<Self, CacheError> {
        let cap = NonZeroUsize::new(capacity).ok_or(CacheError::InvalidCapacity)?;
        Ok(Self::with_capacity(cap))
    }

    /// Creates a cache with the given non-zero capacity. Infallible.
    ///
    /// # Example
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use cache_mod::LruCache;
    ///
    /// let cap = NonZeroUsize::new(64).expect("64 != 0");
    /// let cache: LruCache<String, u32> = LruCache::with_capacity(cap);
    /// ```
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        let num_shards = sharding::shard_count(capacity);
        let per_shard = sharding::per_shard_capacity(capacity, num_shards);
        let sharded = Sharded::from_factory(num_shards, |_| Inner::with_capacity(per_shard));
        Self { capacity, sharded }
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let idx = *inner.map.get(key)?;
        inner.promote(idx);
        inner.nodes[idx].as_ref().map(|n| n.value.clone())
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.sharded.shard_for(&key).lock_recover();

        if let Some(&idx) = inner.map.get(&key) {
            let old = inner.nodes[idx]
                .as_mut()
                .map(|n| core::mem::replace(&mut n.value, value))
                .unwrap_or_else(|| unreachable!("mapped index must be occupied"));
            inner.promote(idx);
            return Some(old);
        }

        // New entry. Evict the LRU tail if at per-shard capacity.
        if inner.map.len() >= inner.capacity.get() {
            if let Some(tail_idx) = inner.tail {
                inner.unlink(tail_idx);
                let evicted = inner.dealloc(tail_idx);
                let _ = inner.map.remove(&evicted.key);
            }
        }

        let idx = inner.alloc(Node {
            key: key.clone(),
            value,
            prev: None,
            next: None,
        });
        inner.push_front(idx);
        let _ = inner.map.insert(key, idx);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.sharded.shard_for(key).lock_recover();
        let idx = inner.map.remove(key)?;
        inner.unlink(idx);
        let node = inner.dealloc(idx);
        Some(node.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.sharded
            .shard_for(key)
            .lock_recover()
            .map
            .contains_key(key)
    }

    fn len(&self) -> usize {
        self.sharded
            .iter()
            .map(|m| m.lock_recover().map.len())
            .sum()
    }

    fn clear(&self) {
        for mutex in self.sharded.iter() {
            let mut inner = mutex.lock_recover();
            inner.nodes.clear();
            inner.free.clear();
            inner.head = None;
            inner.tail = None;
            inner.map.clear();
        }
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}
