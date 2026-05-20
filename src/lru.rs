//! Least-Recently-Used (LRU) cache — arena-backed reference implementation.

use core::hash::Hash;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::util::MutexExt;

/// A bounded, thread-safe LRU cache.
///
/// On insert overflow the least-recently-accessed entry is evicted. Both
/// [`get`](Cache::get) and [`insert`](Cache::insert) count as accesses and
/// promote the affected entry to most-recently-used.
///
/// 0.6.0 implementation: an index-stable arena of nodes plus a `HashMap<K, usize>`
/// keyed lookup. Promote and evict are O(1); the 0.5.x `VecDeque` scan is gone.
/// Lock strategy is still a single `Mutex<Inner>`; the sharded / lock-free
/// upgrade lands in 0.7.0 without changing this public surface.
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
/// cache.insert(3, "three"); // evicts 2
/// assert_eq!(cache.get(&2), None);
/// assert_eq!(cache.get(&1), Some("one"));
/// assert_eq!(cache.get(&3), Some("three"));
/// ```
pub struct LruCache<K, V> {
    capacity: NonZeroUsize,
    inner: Mutex<Inner<K, V>>,
}

struct Node<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

struct Inner<K, V> {
    /// Slab of nodes. `None` slots are on the free list.
    nodes: Vec<Option<Node<K, V>>>,
    /// Free indices in `nodes` available for reuse.
    free: Vec<usize>,
    /// Most-recently-used node index, if any.
    head: Option<usize>,
    /// Least-recently-used node index, if any.
    tail: Option<usize>,
    /// Key → arena index.
    map: HashMap<K, usize>,
}

impl<K, V> Inner<K, V>
where
    K: Eq + Hash + Clone,
{
    fn with_capacity(cap: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(cap),
            free: Vec::new(),
            head: None,
            tail: None,
            map: HashMap::with_capacity(cap),
        }
    }

    /// Allocate an arena slot for `node`, returning its stable index.
    fn alloc(&mut self, node: Node<K, V>) -> usize {
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    /// Free `idx` for reuse and return the owned node.
    fn dealloc(&mut self, idx: usize) -> Node<K, V> {
        let node = self.nodes[idx]
            .take()
            .unwrap_or_else(|| unreachable!("arena slot must be occupied to be deallocated"));
        self.free.push(idx);
        node
    }

    /// Unlink `idx` from the access-order list. Caller must ensure `idx`
    /// is currently in the list.
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

    /// Insert `idx` at the head (most-recently-used position).
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
            // First entry — both head and tail.
            self.tail = Some(idx);
        }
        self.head = Some(idx);
    }

    /// Promote `idx` to head if it's not already there.
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
        let cap = capacity.get();
        Self {
            capacity,
            inner: Mutex::new(Inner::with_capacity(cap)),
        }
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let idx = *inner.map.get(key)?;
        inner.promote(idx);
        let value = inner.nodes[idx]
            .as_ref()
            .map(|n| n.value.clone())
            .unwrap_or_else(|| unreachable!("promoted node must be occupied"));
        Some(value)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let mut inner = self.inner.lock_recover();

        if let Some(&idx) = inner.map.get(&key) {
            // Live update: replace value, promote.
            let old = inner.nodes[idx]
                .as_mut()
                .map(|n| core::mem::replace(&mut n.value, value))
                .unwrap_or_else(|| unreachable!("mapped index must be occupied"));
            inner.promote(idx);
            return Some(old);
        }

        // New entry. Evict the LRU tail if at capacity.
        if inner.map.len() >= self.capacity.get() {
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
        let mut inner = self.inner.lock_recover();
        let idx = inner.map.remove(key)?;
        inner.unlink(idx);
        let node = inner.dealloc(idx);
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
    }

    fn capacity(&self) -> usize {
        self.capacity.get()
    }
}
