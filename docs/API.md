<h1 id="top" align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>cache-mod</b><br>
    <sub><sup>API REFERENCE</sup></sub>
</h1>
<div align="center">
    <sup>
        <a href="../README.md" title="Project Home"><b>HOME</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="./README.md" title="Documentation"><b>DOCS</b></a>
        <span>&nbsp;│&nbsp;</span>
        <span>API</span>
        <span>&nbsp;│&nbsp;</span>
        <a href="../CHANGELOG.md" title="Changelog"><b>CHANGELOG</b></a>
    </sup>
</div>
<br>

This is the complete public API reference for `cache-mod`. Every public item is listed with its signature, contract, and at least one working code example. For the higher-level docs (versions, release notes), see [docs/README.md](./README.md). For machine-rendered rustdoc, see [docs.rs/cache-mod](https://docs.rs/cache-mod).

<br>

## Table of Contents

- **[Installation](#installation)**
- **[Quick Start](#quick-start)**
- **[Choosing a Cache Type](#choosing-a-cache-type)**
- **[Public APIs](#public-apis)**
  - [The `Cache` trait](#the-cache-trait)
  - [`CacheError`](#cacheerror)
  - [`LruCache`](#lrucache)
  - [`LfuCache`](#lfucache)
  - [`TtlCache`](#ttlcache)
  - [`TinyLfuCache`](#tinylfucache)
  - [`SizedCache`](#sizedcache)
  - [`VERSION`](#version)
- **[Cross-cutting Contracts](#cross-cutting-contracts)**
  - [Access semantics](#access-semantics)
  - [Capacity contract](#capacity-contract)
  - [Concurrency](#concurrency)
  - [Poison tolerance](#poison-tolerance)
- **[Real-World Examples](#real-world-examples)**
  - [HTTP response cache (LRU)](#http-response-cache-lru)
  - [Computed-result cache with skew (LFU)](#computed-result-cache-with-skew-lfu)
  - [Session store (TTL)](#session-store-ttl)
  - [Hot-key admission (TinyLFU)](#hot-key-admission-tinylfu)
  - [Byte-budgeted image cache (SizedCache)](#byte-budgeted-image-cache-sizedcache)
- **[Notes](#notes)**

<br><br>

## Installation

#### Install manually

Add this to your `Cargo.toml`:

```toml
[dependencies]
cache-mod = "0.5"
```

#### Install via terminal

```bash
cargo add cache-mod
```

**MSRV:** Rust `1.75`. **Edition:** `2021`. **Default features:** `std`.

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

## Quick Start

```rust
use cache_mod::{Cache, LruCache};

let cache: LruCache<&'static str, u32> = LruCache::new(64).expect("capacity > 0");

cache.insert("requests", 1);
cache.insert("errors", 0);

assert_eq!(cache.get(&"requests"), Some(1));
assert_eq!(cache.len(), 2);
assert_eq!(cache.capacity(), 64);
```

Every cache type in this crate implements the same [`Cache`](#the-cache-trait) trait, so the call surface above (`insert` / `get` / `len` / `capacity` / `remove` / `contains_key` / `clear` / `is_empty`) is identical across `LruCache`, `LfuCache`, `TtlCache`, `TinyLfuCache`, and `SizedCache`. Pick the type whose eviction policy fits your access pattern; the call sites won't change.

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

## Choosing a Cache Type

| Type            | Eviction policy                                                 | Best for                                                | Notable contract                                                            |
| --------------- | --------------------------------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------- |
| `LruCache`      | Least-Recently-Used                                             | Working sets with recency-of-access locality            | `get` and `insert` promote to MRU                                           |
| `LfuCache`      | Least-Frequently-Used (ties broken by LRU)                      | Stable hot-set; per-key access counts matter            | Counter resets on eviction; `contains_key` does **not** increment           |
| `TtlCache`      | Time-To-Live, lazy expiry; evicts soonest-expiring on overflow  | Per-entry lifetimes (sessions, signed URLs, rate cards) | `insert` resets the deadline; an expired re-insert returns `None`, not the stale value |
| `TinyLfuCache`  | Count-Min Sketch admission + LRU main                           | High write-pressure workloads where pollution matters   | **`insert` may not persist** — admission filter can reject cold candidates  |
| `SizedCache`    | Byte-weight bound, LRU within the bound                         | Heterogeneous value sizes (images, payloads, blobs)     | `capacity()` returns `max_weight`; values larger than `max_weight` silently rejected |

All five caches share the same `Send + Sync` contract, the same poison-tolerant `Mutex` recovery, and the same MSRV.

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

## Public APIs

### The `Cache` trait

The common read / write / evict contract every cache type in this crate implements.

```rust
pub trait Cache<K, V>
where
    K: Eq + core::hash::Hash,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V>;
    fn insert(&self, key: K, value: V) -> Option<V>;
    fn remove(&self, key: &K) -> Option<V>;
    fn contains_key(&self, key: &K) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;                 // default impl: self.len() == 0
    fn clear(&self);
    fn capacity(&self) -> usize;
}
```

#### `get(&self, key: &K) -> Option<V>`

Returns the value associated with `key`, if any. Calling `get` **is an access** for the purposes of the eviction policy: it may promote the entry to MRU (`LruCache`, `TinyLfuCache`, `SizedCache`), bump its frequency counter (`LfuCache`), update its access timestamp (`LfuCache`, `TinyLfuCache`), or trigger lazy expiry cleanup (`TtlCache`).

```rust
use cache_mod::{Cache, LruCache};
let cache: LruCache<u32, &str> = LruCache::new(4).expect("capacity > 0");
cache.insert(1, "one");
assert_eq!(cache.get(&1), Some("one"));
assert_eq!(cache.get(&999), None);
```

#### `insert(&self, key: K, value: V) -> Option<V>`

Inserts `value` under `key`. Returns the **previous** value if `key` was already present.

- For `LruCache` / `LfuCache` / `TtlCache` / `SizedCache`: insert is unconditional. The cache makes room by eviction if needed.
- For `TinyLfuCache`: insert is subject to the admission filter. At capacity, a new key is admitted only if the Count-Min Sketch frequency estimate for the incoming key exceeds the LRU victim's. Rejected admissions return `None` and silently drop the value.
- For `TtlCache`: writes always reset the deadline on the affected entry — `insert` and `insert_with_ttl` re-arm the timer.

```rust
use cache_mod::{Cache, LruCache};
let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
assert_eq!(cache.insert(1, 10), None);       // new key
assert_eq!(cache.insert(1, 20), Some(10));   // returns the old value
```

#### `remove(&self, key: &K) -> Option<V>`

Removes the entry for `key` and returns the value if it was present. Destructive; does not update eviction order beyond removing the entry.

```rust
use cache_mod::{Cache, LruCache};
let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
cache.insert(1, 10);
assert_eq!(cache.remove(&1), Some(10));
assert_eq!(cache.remove(&1), None);          // already gone
```

#### `contains_key(&self, key: &K) -> bool`

Returns `true` if the cache currently holds an entry for `key`. Unlike `get`, this method does **not** count as an access — the eviction order, frequency counters, and access timestamps are left untouched.

```rust
use cache_mod::{Cache, LruCache};
let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
cache.insert(1, 10);
assert!(cache.contains_key(&1));
// `contains_key` did not promote 1 to MRU — the policy still treats it
// as the least-recently-used entry.
```

For `TtlCache`, `contains_key` performs lazy expiry: an expired entry is removed during the check, and the method then returns `false`.

#### `len(&self) -> usize` / `is_empty(&self) -> bool`

`len` reports the number of currently-stored entries. `TtlCache::len` runs a sweep first, so the returned value is the live count (expired entries are dropped). For `SizedCache`, `len` is entry count — use [`total_weight()`](#sizedcache-total_weight) for byte usage.

```rust
use cache_mod::{Cache, LruCache};
let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
assert!(cache.is_empty());
cache.insert(1, 10);
assert_eq!(cache.len(), 1);
assert!(!cache.is_empty());
```

#### `clear(&self)`

Removes every entry. For `LfuCache` / `TinyLfuCache`, the internal counters and sketch are also reset. Capacity itself is preserved.

```rust
use cache_mod::{Cache, LruCache};
let cache: LruCache<u32, u32> = LruCache::new(4).expect("capacity > 0");
cache.insert(1, 10);
cache.insert(2, 20);
cache.clear();
assert!(cache.is_empty());
assert_eq!(cache.capacity(), 4);             // capacity is unchanged
```

#### `capacity(&self) -> usize`

Returns the configured capacity bound. The unit depends on the implementation:

- `LruCache`, `LfuCache`, `TtlCache`, `TinyLfuCache` — maximum number of entries.
- `SizedCache` — maximum total byte-weight across entries (same value as `max_weight()`).

<br>

### `CacheError`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheError {
    InvalidCapacity,
}
```

The single variant `InvalidCapacity` is returned by every fallible constructor (`LruCache::new`, `LfuCache::new`, `TtlCache::new`, `TinyLfuCache::new`, `SizedCache::new`) when the requested capacity is zero. The enum is `#[non_exhaustive]`; new variants may be added in minor releases.

Implements: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Display`. The `std::error::Error` impl is gated on the `std` feature (the default).

```rust
use cache_mod::{CacheError, LruCache};
let err = LruCache::<u32, u32>::new(0).err();
assert_eq!(err, Some(CacheError::InvalidCapacity));
```

<br>

### `LruCache`

Bounded, thread-safe Least-Recently-Used cache. On overflow, the entry that was least-recently accessed is evicted. Both `get` and `insert` promote the affected entry to most-recently-used; `contains_key` does not.

```rust
pub struct LruCache<K, V> { /* opaque */ }

impl<K: Eq + Hash + Clone, V: Clone> LruCache<K, V> {
    pub fn new(capacity: usize) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize) -> Self;
}
```

#### `LruCache::new(capacity: usize) -> Result<Self, CacheError>`

Fallible constructor. Returns `CacheError::InvalidCapacity` if `capacity == 0`.

```rust
use cache_mod::LruCache;
let cache: LruCache<String, u32> = LruCache::new(128).expect("capacity > 0");
```

#### `LruCache::with_capacity(capacity: NonZeroUsize) -> Self`

Infallible constructor for callers that already hold a `NonZeroUsize`.

```rust
use std::num::NonZeroUsize;
use cache_mod::LruCache;
let cap = NonZeroUsize::new(64).expect("64 != 0");
let cache: LruCache<String, u32> = LruCache::with_capacity(cap);
```

Full LRU example covering eviction:

```rust
use cache_mod::{Cache, LruCache};

let cache: LruCache<u32, &str> = LruCache::new(2).expect("capacity > 0");
cache.insert(1, "one");
cache.insert(2, "two");

// Access 1 — 1 becomes MRU, 2 becomes LRU.
assert_eq!(cache.get(&1), Some("one"));

// Inserting 3 evicts 2 (LRU).
cache.insert(3, "three");
assert_eq!(cache.get(&2), None);
assert_eq!(cache.get(&1), Some("one"));
assert_eq!(cache.get(&3), Some("three"));
```

<br>

### `LfuCache`

Bounded, thread-safe Least-Frequently-Used cache. Each entry carries a counter that increments on every `get` or `insert` of an already-present key. On overflow, the entry with the lowest counter is evicted; ties are broken in favour of the least-recently-accessed entry.

```rust
pub struct LfuCache<K, V> { /* opaque */ }

impl<K: Eq + Hash + Clone, V: Clone> LfuCache<K, V> {
    pub fn new(capacity: usize) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize) -> Self;
}
```

Constructors mirror [`LruCache`](#lrucache) exactly.

#### LFU eviction with LRU tie-break

```rust
use cache_mod::{Cache, LfuCache};

let cache: LfuCache<u32, u32> = LfuCache::new(2).expect("capacity > 0");
cache.insert(1, 10);
cache.insert(2, 20);

// Bump key 1's counter above key 2's.
assert_eq!(cache.get(&1), Some(10));

// Inserting 3 evicts 2 (lowest counter).
cache.insert(3, 30);
assert_eq!(cache.get(&2), None);
assert_eq!(cache.get(&1), Some(10));
assert_eq!(cache.get(&3), Some(30));
```

If two entries share the minimum counter (e.g. both have been accessed once), the older entry is evicted first — `LfuCache` keeps the fresher of two equally-cold entries.

<br>

### `TtlCache`

Bounded, thread-safe cache with per-entry time-to-live. Each entry is stamped with a deadline at insert time. Expired entries are removed lazily during `get`, `contains_key`, and `len`. On overflow, the entry with the **soonest expiration** is evicted, naturally preferring already-expired entries over live ones.

```rust
pub struct TtlCache<K, V> { /* opaque */ }

impl<K: Eq + Hash + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(capacity: usize, ttl: Duration) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize, ttl: Duration) -> Self;
    pub fn insert_with_ttl(&self, key: K, value: V, ttl: Duration) -> Option<V>;
}
```

#### `TtlCache::new(capacity: usize, ttl: Duration) -> Result<Self, CacheError>`

`ttl` is the **default** time-to-live applied to every `insert` that doesn't specify its own. Returns `CacheError::InvalidCapacity` if `capacity == 0`.

```rust
use std::time::Duration;
use cache_mod::TtlCache;

let cache: TtlCache<String, u32> =
    TtlCache::new(128, Duration::from_secs(300)).expect("capacity > 0");
```

#### `TtlCache::insert_with_ttl(&self, key: K, value: V, ttl: Duration) -> Option<V>`

Per-call TTL override. The deadline is `now + ttl`. Returns the previously-stored live value if `key` was present and not yet expired; otherwise returns `None` (an expired-but-not-yet-cleaned entry is treated as absent).

```rust
use std::time::Duration;
use cache_mod::{Cache, TtlCache};

let cache: TtlCache<u32, u32> =
    TtlCache::new(4, Duration::from_secs(60)).expect("capacity > 0");

cache.insert_with_ttl(1, 10, Duration::from_secs(5));
assert_eq!(cache.get(&1), Some(10));
```

**TTL overflow guard.** `now + ttl` is computed with `Instant::checked_add`. If the addition would overflow (e.g. `Duration::MAX`), the deadline is clamped to roughly 100 years from now. No panics on absurd input.

<br>

### `TinyLfuCache`

A bounded, thread-safe cache with **admission control**. Every key the cache observes — hit or miss — feeds a fixed-size Count-Min Sketch. On capacity overflow, the incoming key is **admitted only if its sketch frequency exceeds the LRU victim's**; one-hit-wonders are rejected at the door instead of displacing hot entries.

```rust
pub struct TinyLfuCache<K, V> { /* opaque */ }

impl<K: Eq + Hash + Clone, V: Clone> TinyLfuCache<K, V> {
    pub fn new(capacity: usize) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize) -> Self;
}
```

**Important contract deviation.** A successful `insert` call **does not guarantee** the value is in the cache. The admission filter may reject it. If your code path needs strict insertion guarantees, use `LruCache` or `LfuCache`.

Sketch parameters (reference implementation):

- depth-4 Count-Min Sketch, `u8` saturating counters
- width = `max(64, 2 × capacity)` rounded to the next power of two
- W-TinyLFU "aging" step: every `10 × capacity` increments, every counter is right-shifted by 1 — keeps the sketch responsive to workload shifts

```rust
use cache_mod::{Cache, TinyLfuCache};

let cache: TinyLfuCache<&str, u32> = TinyLfuCache::new(256).expect("capacity > 0");

// Build up the frequency signal for "hot" before the cache fills.
for _ in 0..32 {
    let _ = cache.get(&"hot");
    let _ = cache.insert("hot", 1);
}

assert_eq!(cache.get(&"hot"), Some(1));
```

<br>

### `SizedCache`

A cache bounded by **total byte-weight** rather than entry count. Each value is weighed at insert time by a user-supplied `fn(&V) -> usize` weigher. Eviction uses LRU semantics until the new entry fits.

```rust
pub struct SizedCache<K, V> { /* opaque */ }

impl<K: Eq + Hash + Clone, V: Clone> SizedCache<K, V> {
    pub fn new(max_weight: usize, weigher: fn(&V) -> usize)
        -> Result<Self, CacheError>;
    pub fn max_weight(&self) -> usize;
    pub fn total_weight(&self) -> usize;
}
```

#### `SizedCache::new(max_weight: usize, weigher: fn(&V) -> usize) -> Result<Self, CacheError>`

Returns `CacheError::InvalidCapacity` if `max_weight == 0`. The weigher is a plain function pointer — captured-state closures would force `Box<dyn Fn>` indirection on every weigh call. If your weighing logic needs state, hoist it into the value type.

```rust
use cache_mod::{Cache, SizedCache};

fn weigh(payload: &Vec<u8>) -> usize { payload.len() }

let cache: SizedCache<&str, Vec<u8>> =
    SizedCache::new(1024, weigh).expect("max_weight > 0");

cache.insert("payload", vec![0u8; 64]);
assert_eq!(cache.total_weight(), 64);
```

#### <span id="sizedcache-max_weight"></span>`SizedCache::max_weight(&self) -> usize`

The configured byte-weight ceiling — same value as `Cache::capacity` for this type.

#### <span id="sizedcache-total_weight"></span>`SizedCache::total_weight(&self) -> usize`

The current sum of weights across live entries.

**Oversized values are silently rejected.** An entry whose own weight exceeds `max_weight` cannot be cached by any policy — `insert` returns `None` and the value is dropped.

```rust
use cache_mod::{Cache, SizedCache};

fn weigh(v: &Vec<u8>) -> usize { v.len() }
let cache: SizedCache<u32, Vec<u8>> = SizedCache::new(100, weigh).expect("max_weight > 0");

// 200 bytes won't fit in a 100-byte cache. Drop silently.
assert_eq!(cache.insert(1, vec![0u8; 200]), None);
assert!(!cache.contains_key(&1));
```

<br>

### `VERSION`

```rust
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

The crate's version string, populated by Cargo at build time. Useful for runtime diagnostics ("which cache-mod is this?") without taking a dependency on `cargo_metadata` or similar.

```rust
assert!(!cache_mod::VERSION.is_empty());
```

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

## Cross-cutting Contracts

### Access semantics

Each `Cache` method has documented access semantics that hold across every implementation:

- `get` is an **access** — may promote, bump a counter, update a timestamp, or trigger lazy expiry.
- `insert` is an **access on the inserted key** plus a possible eviction trigger.
- `contains_key` is a **query** — must not promote, bump, or shift access order. (TTL is the one nuance: `contains_key` may *remove* an expired entry, because reporting `true` for a dead entry would be wrong.)
- `remove` is destructive; does not update order.
- `clear` resets the cache to its post-construction state (entries gone, capacity preserved, sketch / counters / clocks reset).

### Capacity contract

For four of the five types, capacity is an entry count and the invariant `cache.len() <= cache.capacity()` holds after every operation. For `SizedCache`, the invariant is `cache.total_weight() <= cache.max_weight()`; entry count can transiently exceed `max_weight` only if the weigher returns zero for some values (a degenerate but legal case).

Both invariants are covered by `proptest`-driven property tests in `tests/properties.rs`.

### Concurrency

Every cache type is `Send + Sync` when `K: Send` and `V: Send` (and similarly for `Sync`). Methods take `&self`, so a single instance can be shared across threads — or held across `.await` points — without external locking. Internally each cache uses a single `Mutex<Inner>` in the 0.5 line; sharded or lock-free internals land in 0.6.0 without changing this public surface.

```rust
use std::sync::Arc;
use std::thread;
use cache_mod::{Cache, LruCache};

let cache: Arc<LruCache<u32, u32>> = Arc::new(LruCache::new(64).expect("capacity > 0"));
let handles: Vec<_> = (0..8u32).map(|i| {
    let cache = Arc::clone(&cache);
    thread::spawn(move || {
        cache.insert(i, i * 10);
        cache.get(&i)
    })
}).collect();
for h in handles { let _ = h.join(); }
```

### Poison tolerance

If a panic occurs while a cache method holds the inner `Mutex`, the lock is *poisoned*. Every cache type recovers automatically: the next call calls `PoisonError::into_inner` and proceeds. This is intentional — every operation re-establishes consistency between `map` and the auxiliary order/sketch/clock state before returning, so a poisoned lock does not weaken the cache's invariants.

Practical implication: a panic in user code that runs while holding a cached value (e.g. inside the value type's `Clone` impl) does not require restarting the cache.

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

## Real-World Examples

### HTTP response cache (LRU)

A simple in-process response cache for an HTTP server. Pages with no per-request variation are cached for the duration of the process; LRU keeps the working set bounded.

```rust
use std::sync::Arc;
use cache_mod::{Cache, LruCache};

#[derive(Clone)]
struct Response {
    status: u16,
    body: String,
}

fn make_cache() -> Arc<LruCache<String, Response>> {
    Arc::new(LruCache::new(4096).expect("capacity > 0"))
}

fn serve(cache: &LruCache<String, Response>, path: &str) -> Response {
    if let Some(cached) = cache.get(&path.to_string()) {
        return cached;
    }
    let resp = Response { status: 200, body: format!("rendered {}", path) };
    cache.insert(path.to_string(), resp.clone());
    resp
}
```

### Computed-result cache with skew (LFU)

Expensive computations where a few inputs are queried far more often than the rest benefit from frequency-aware eviction. `LfuCache` keeps the hot set even if the cold set is recently touched.

```rust
use cache_mod::{Cache, LfuCache};

fn expensive(input: u64) -> u64 {
    // imagine a multi-millisecond computation here
    input.wrapping_mul(2654435761)
}

let cache: LfuCache<u64, u64> = LfuCache::new(512).expect("capacity > 0");

fn lookup(cache: &LfuCache<u64, u64>, input: u64) -> u64 {
    if let Some(v) = cache.get(&input) { return v; }
    let v = expensive(input);
    cache.insert(input, v);
    v
}
```

### Session store (TTL)

Web session stores want time-to-live, not entry-count eviction. Sessions expire after their inactivity window; the bounded capacity prevents unbounded growth.

```rust
use std::time::Duration;
use cache_mod::{Cache, TtlCache};

#[derive(Clone)]
struct Session { user_id: u64, csrf_token: String }

let sessions: TtlCache<String, Session> =
    TtlCache::new(10_000, Duration::from_secs(30 * 60))      // default 30 min
        .expect("capacity > 0");

// Long-lived "remember me" session — override the default TTL.
sessions.insert_with_ttl(
    "rm-cookie".to_string(),
    Session { user_id: 42, csrf_token: "...".to_string() },
    Duration::from_secs(30 * 24 * 60 * 60),                  // 30 days
);
```

### Hot-key admission (TinyLFU)

Workloads with a long-tail of one-off keys (request scans, broken clients, log replays) can pollute an LRU cache by displacing legitimately hot entries. `TinyLfuCache`'s admission filter rejects keys whose sketch frequency hasn't risen above the existing victim's.

```rust
use cache_mod::{Cache, TinyLfuCache};

let cache: TinyLfuCache<u64, Vec<u8>> = TinyLfuCache::new(1024).expect("capacity > 0");

fn observe(cache: &TinyLfuCache<u64, Vec<u8>>, id: u64, blob: Vec<u8>) -> Option<Vec<u8>> {
    // Both `get` (miss case) and `insert` feed the sketch — repeated
    // accesses to the same id raise its admission score even before it
    // is in the cache.
    if let Some(v) = cache.get(&id) { return Some(v); }
    let _ = cache.insert(id, blob);
    cache.get(&id)              // None means admission was rejected
}
```

### Byte-budgeted image cache (SizedCache)

When values have very different sizes (small thumbnails next to large hero images), entry-count caps either waste memory (sized for the worst case) or run out (sized for the average). `SizedCache` bounds total weight instead.

```rust
use cache_mod::{Cache, SizedCache};

#[allow(clippy::ptr_arg)]                       // weigher signature must match V
fn image_bytes(img: &Vec<u8>) -> usize { img.len() }

let cache: SizedCache<String, Vec<u8>> =
    SizedCache::new(64 * 1024 * 1024, image_bytes)     // 64 MiB ceiling
        .expect("max_weight > 0");

cache.insert("hero.png".to_string(), vec![0u8; 8 * 1024 * 1024]);  // 8 MiB
cache.insert("thumb-1.png".to_string(), vec![0u8; 16 * 1024]);    // 16 KiB

assert!(cache.total_weight() <= cache.max_weight());
```

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

## Notes

- **Pre-1.0 API.** The public surface described above is feature-complete as of 0.5.0 but not yet frozen. Minor versions may break in the future. Pin exact versions and read the [CHANGELOG](../CHANGELOG.md).
- **Lock-free internals.** All five cache types use `std::sync::Mutex` for internal coordination in the 0.5 line. Arena-backed, O(1) internals (and a sharded-lock or `crossbeam-epoch` lock-strategy upgrade) land in 0.6.0 without changing the public surface documented here.
- **REPS.** Every public item is covered by rustdoc with at least one example. No `unsafe` is used anywhere in the crate. See [REPS.md](../REPS.md) for the project's quality discipline.
- **Tests.** The crate ships 47 integration tests, 9 property tests (`proptest`), and 18 doctests — every public method has a working example that runs under `cargo test`.

<hr><br>
<a href="#top">&uarr; <b>TOP</b></a>
<br>

<div align="center">
  <sub>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sub>
</div>
