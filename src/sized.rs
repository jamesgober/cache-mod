//! Byte-bound cache — capacity is total weight across entries, not entry count.

use core::hash::Hash;
use std::collections::{HashMap, VecDeque};
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

struct Entry<V> {
    value: V,
    weight: usize,
}

struct Inner<K, V> {
    map: HashMap<K, Entry<V>>,
    /// Access order. Front = most-recently-used, back = least-recently-used.
    order: VecDeque<K>,
    total_weight: usize,
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
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                order: VecDeque::new(),
                total_weight: 0,
            }),
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
        let value = inner.map.get(key)?.value.clone();
        promote(&mut inner.order, key);
        Some(value)
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        let new_weight = (self.weigher)(&value);
        // Reject values that can't possibly fit. Drop them silently — the
        // alternative would be a new error variant for a degenerate input.
        if new_weight > self.max_weight {
            return None;
        }

        let mut inner = self.inner.lock_recover();

        // Live update path: replace value, fix up total_weight, return old.
        if let Some(existing) = inner.map.get_mut(&key) {
            let old_value = core::mem::replace(&mut existing.value, value);
            let old_weight = existing.weight;
            existing.weight = new_weight;
            // total_weight delta = new - old (may go negative; both unsigned, so
            // compute as add-then-sub via saturating arithmetic).
            inner.total_weight = inner
                .total_weight
                .saturating_add(new_weight)
                .saturating_sub(old_weight);
            promote(&mut inner.order, &key);
            evict_until_fits(&mut inner, self.max_weight);
            return Some(old_value);
        }

        // New entry — make room first, then insert.
        let projected_total = inner.total_weight.saturating_add(new_weight);
        if projected_total > self.max_weight {
            evict_until_fits_for_new(&mut inner, self.max_weight, new_weight);
        }
        inner.order.push_front(key.clone());
        let _ = inner.map.insert(
            key,
            Entry {
                value,
                weight: new_weight,
            },
        );
        inner.total_weight = inner.total_weight.saturating_add(new_weight);
        None
    }

    fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock_recover();
        let entry = inner.map.remove(key)?;
        inner.total_weight = inner.total_weight.saturating_sub(entry.weight);
        if let Some(pos) = inner.order.iter().position(|k| k == key) {
            let _ = inner.order.remove(pos);
        }
        Some(entry.value)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.inner.lock_recover().map.contains_key(key)
    }

    fn len(&self) -> usize {
        self.inner.lock_recover().map.len()
    }

    fn clear(&self) {
        let mut inner = self.inner.lock_recover();
        inner.map.clear();
        inner.order.clear();
        inner.total_weight = 0;
    }

    /// For `SizedCache`, capacity is the configured `max_weight` — see the
    /// type-level docs.
    fn capacity(&self) -> usize {
        self.max_weight
    }
}

fn promote<K: Eq>(order: &mut VecDeque<K>, key: &K) {
    if let Some(pos) = order.iter().position(|k| k == key) {
        if let Some(k) = order.remove(pos) {
            order.push_front(k);
        }
    }
}

/// Evict from the LRU end until `total_weight` is within `max_weight`.
/// Used on live update where the new weight might overshoot.
fn evict_until_fits<K, V>(inner: &mut Inner<K, V>, max_weight: usize)
where
    K: Eq + Hash,
{
    while inner.total_weight > max_weight {
        let Some(victim_key) = inner.order.pop_back() else {
            break;
        };
        if let Some(victim) = inner.map.remove(&victim_key) {
            inner.total_weight = inner.total_weight.saturating_sub(victim.weight);
        }
    }
}

/// Variant for new-entry inserts: evict until the projected total (current plus
/// incoming weight) fits. Separated from `evict_until_fits` because the trigger
/// condition is different.
fn evict_until_fits_for_new<K, V>(inner: &mut Inner<K, V>, max_weight: usize, incoming: usize)
where
    K: Eq + Hash,
{
    while inner.total_weight.saturating_add(incoming) > max_weight {
        let Some(victim_key) = inner.order.pop_back() else {
            break;
        };
        if let Some(victim) = inner.map.remove(&victim_key) {
            inner.total_weight = inner.total_weight.saturating_sub(victim.weight);
        }
    }
}
