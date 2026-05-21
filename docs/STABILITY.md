<h1 id="top" align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>cache-mod</b><br>
    <sub><sup>STABILITY PROMISE</sup></sub>
</h1>
<div align="center">
    <sup>
        <a href="../README.md" title="Project Home"><b>HOME</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="./README.md" title="Documentation"><b>DOCS</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="./API.md" title="API Reference"><b>API</b></a>
        <span>&nbsp;│&nbsp;</span>
        <span>STABILITY</span>
    </sup>
</div>
<br>

This document enumerates every public symbol of `cache-mod` at the 1.0.0 release and the SemVer commitments that come with it. If a symbol appears here, it is part of the frozen surface — changing it requires a 2.0 release. If it isn't listed, it isn't promised.

## Strict SemVer commitment

Once 1.0.0 ships, every item listed below is governed by strict SemVer:

- **Removing, renaming, or changing the signature** of a frozen symbol → requires a **2.0** release.
- **Adding** new symbols (variants, methods, types, modules, feature flags, trait methods with default implementations) → may land in a **minor** release.
- **Bug fixes** that change observable behaviour to match documented contracts → may land in a **patch** release.

Behaviour that is *not* part of the frozen surface (see "Explicitly not promised" below) may change freely within the 1.x line.

## Frozen surface

### `Cache` trait

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
    fn is_empty(&self) -> bool;       // default impl: self.len() == 0
    fn clear(&self);
    fn capacity(&self) -> usize;
}
```

Every method signature listed above is frozen. The `#[must_use]` attributes on `get`, `contains_key`, `len`, `is_empty`, and `capacity` are also part of the contract — they will not be removed.

Adding new trait methods is allowed in a minor release **only if** the new method has a default implementation that does not require existing implementors to opt in. Trait-level type parameters and bounds are frozen.

### `CacheError`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheError {
    InvalidCapacity,
}
```

The enum is `#[non_exhaustive]`. New variants may be added in minor releases. The `InvalidCapacity` variant is frozen.

`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Display`, and (under the `std` feature) `std::error::Error` impls are frozen.

### Cache types

All five concrete cache types are frozen at the type level:

```rust
pub struct LruCache<K, V> { /* opaque */ }
pub struct LfuCache<K, V> { /* opaque */ }
pub struct TtlCache<K, V> { /* opaque */ }
pub struct TinyLfuCache<K, V> { /* opaque */ }
pub struct SizedCache<K, V> { /* opaque */ }
```

Constructors and inherent methods, frozen:

```rust
impl<K, V> LruCache<K, V>
where K: Eq + Hash + Clone, V: Clone,
{
    pub fn new(capacity: usize) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize) -> Self;
}

impl<K, V> LfuCache<K, V>
where K: Eq + Hash + Clone, V: Clone,
{
    pub fn new(capacity: usize) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize) -> Self;
}

impl<K, V> TtlCache<K, V>
where K: Eq + Hash + Clone, V: Clone,
{
    pub fn new(capacity: usize, ttl: Duration) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize, ttl: Duration) -> Self;
    pub fn insert_with_ttl(&self, key: K, value: V, ttl: Duration) -> Option<V>;
}

impl<K, V> TinyLfuCache<K, V>
where K: Eq + Hash + Clone, V: Clone,
{
    pub fn new(capacity: usize) -> Result<Self, CacheError>;
    pub fn with_capacity(capacity: NonZeroUsize) -> Self;
}

impl<K, V> SizedCache<K, V>
where K: Eq + Hash + Clone, V: Clone,
{
    pub fn new(max_weight: usize, weigher: fn(&V) -> usize) -> Result<Self, CacheError>;
    pub fn max_weight(&self) -> usize;
    pub fn total_weight(&self) -> usize;
}
```

Every cache type implements `Cache<K, V>` for matching `K, V` bounds — frozen.

### `Send + Sync` impls

For every cache type, the following holds and is frozen:

- `LruCache<K, V>: Send` when `K: Send, V: Send`
- `LruCache<K, V>: Sync` when `K: Send, V: Send`

(Identical predicates for `LfuCache`, `TtlCache`, `TinyLfuCache`, `SizedCache`.)

The `Send` and `Sync` impls will not be tightened or loosened in the 1.x line.

### `VERSION` constant

```rust
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

Frozen. Will always be the build-time crate version string.

### Feature flags

The crate exposes one feature flag, frozen:

- `default = ["std"]`
- `std` — opt out for `no_std` builds. The `Cache` trait and `CacheError` compile without `std`; the five concrete cache types require `std` (they use `HashMap`, `Mutex`, etc.).

New feature flags may be added in minor releases. The `std` flag will not be renamed or removed.

## Behavioural contracts

The following observable behaviours are part of the contract and require a 2.0 release to change:

- **`Cache::get` is an access** — promotes (LRU/TinyLFU/SizedCache), bumps frequency (LFU/TinyLFU), updates access timestamp (LFU/TinyLFU), or triggers lazy expiry cleanup (TTL).
- **`Cache::contains_key` is a query** — does not promote, bump, or shift access order. The one TTL-specific nuance is that an expired-but-not-yet-cleaned entry is removed during the check and the method returns `false`.
- **`Cache::insert` returns the previously-stored value** if the key was already present, except on `TinyLfuCache` where admission rejection silently drops the new value and returns `None`.
- **`Cache::clear` resets auxiliary state** — `LfuCache`'s priority index, `TinyLfuCache`'s Count-Min Sketch, the monotonic clocks — alongside the entries.
- **Capacity invariant** — after every operation, `cache.len() <= cache.capacity()` for entry-bounded caches, and `cache.total_weight() <= cache.max_weight()` for `SizedCache`. With sharded caches this is a per-shard invariant that sums to the global bound, modulo integer-division residue (documented per type).
- **`LruCache`** evicts the least-recently-accessed entry on overflow (per-shard once the cache is sharded — see below).
- **`LfuCache`** evicts the lowest-counter entry on overflow, breaking ties by least-recently-accessed.
- **`TtlCache`** evicts the soonest-expiring entry on overflow; expired entries are removed lazily on access.
- **`TinyLfuCache`** admits an incoming key on overflow only if its sketch-estimated frequency exceeds the LRU victim's; rejected admissions return `None`.
- **`SizedCache`** evicts least-recently-accessed entries until the new entry fits within `max_weight`. Values whose own weight exceeds `max_weight` (or the per-shard ceiling, when applicable) are silently rejected.
- **`CacheError::InvalidCapacity`** is returned by every fallible constructor when capacity (or `max_weight` for `SizedCache`) is zero.
- **No `unsafe`, no `panic!`, no `unwrap`, no `expect`** on shipping paths. No background threads. No required async runtime.
- **`Send + Sync`** as documented above.

### Sharded eviction is approximate (by design)

Once a cache holds at least 32 entries (for entry-bounded caches) or 32 bytes (for `SizedCache`), the internal storage is sharded into up to 16 independent stores. Eviction within an overflowing shard picks the local-to-shard candidate (least-recently-accessed, lowest-counter, soonest-expiring, etc.) — **not** the global candidate. Hit-rate impact for well-distributed keys is statistically negligible; the eviction-precision trade-off is the contract.

`SizedCache` is currently unsharded; that may change in a future release. When it does change, it will keep the existing weight-budget contract (no value larger than `max_weight` is ever admitted) regardless of internal layout.

## MSRV (Minimum Supported Rust Version)

- **Frozen at 1.0.0: Rust 1.75.**
- MSRV bumps within the 1.x line are advertised in the CHANGELOG and are **not** treated as breaking. A 1.x release is allowed to require a newer Rust toolchain than 1.75, but downstream pinning of an older `cache-mod` minor will keep working on the older Rust.
- A 2.0 release may raise MSRV more aggressively.

Edition is 2021 and will stay 2021 within the 1.x line.

## Explicitly **not** promised

The following are **not** part of the frozen surface and may change freely in any 1.x release:

- **Exact eviction order** beyond what the behavioural contracts above commit to. "Approximate" is sufficient.
- **Internal data structures.** Arena layouts, BTreeMap priority indices, Count-Min Sketch parameters (depth, width, aging cadence) are all implementation details.
- **Shard count and the sharding heuristic.** The "16 shards capped, 32-entry single-shard threshold" numbers are tunable.
- **Performance characteristics.** Asymptotic improvements may happen freely; benchmark numbers are not part of the contract.
- **Error message text.** `Display` output for `CacheError` may evolve. Match on variants, not strings.
- **Hash function used for shard routing.** `DefaultHasher` today; might change to a faster hasher tomorrow.
- **Anything `pub(crate)` or `#[doc(hidden)]`.** Internal-only.
- **The exact list of dev-dependencies.** `criterion`, `proptest` are dev-only; users don't see them.
- **The `Cargo.lock` contents.** This is a library — only the manifest matters.

## When a 2.0 might happen

The library is "feature-complete" as of 0.5.0. Reasons that would justify a 2.0:

- A meaningfully-incompatible eviction policy change (e.g. switching `TinyLfuCache` to full W-TinyLFU with SLRU+doorkeeper if the surface had to change to expose it).
- A `Borrow<Q>`-generic lookup API on `Cache::get` / `contains_key` / `remove` (this is additive in some forms, but if it forces a trait signature change it is breaking).
- Replacing `fn(&V) -> usize` weighers on `SizedCache` with a generic `Weigh<V>` trait, if there is sufficient demand for state-carrying weighers.
- An MSRV jump large enough to force the toolchain decision (e.g. requiring `let-else` features that 1.75 doesn't support — though 1.75 already supports the patterns the crate uses).

None of these is on the roadmap as of 1.0.0.

---

<div align="center">
  <sub>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sub>
</div>
