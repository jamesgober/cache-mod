# Changelog

All notable changes to `cache-mod` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [1.0.0] - 2026-05-21

API freeze. The public surface that landed across 0.2.0 – 0.9.0 is now committed under strict SemVer per [`docs/STABILITY.md`](docs/STABILITY.md). Library code is byte-identical to 0.9.0 — every existing call-site compiles and behaves the same.

### Added

- `docs/STABILITY.md` — the stability promise. Enumerates every committed symbol (the `Cache` trait, `CacheError`, the five cache types and their constructors / methods, `VERSION`), the behavioural contracts (access semantics, capacity invariant, eviction rules, `Send + Sync` predicates, no-`unsafe` / no-`panic` guarantees), MSRV policy, feature-flag stability, and an explicit "not promised" list of internals that may evolve in the 1.x line.

### Changed

- Crate-level rustdoc (`src/lib.rs`) Status section updated to declare the API frozen as of 1.0.0; no more pre-1.0 caveats.
- `README.md`, `docs/API.md` updated to point at `STABILITY.md` and recommend `cache-mod = "1"` pinning. Pre-1.0 language ("not yet frozen", "expect breaking changes", "0.5 line") removed.
- `docs/API.md`: expanded examples — each cache type now has multiple worked use-case examples beyond the one-shot reference (LRU: ordering + concurrent + replacement; LFU: ordering + tie-break + non-promotion; TTL: per-call override + lazy expiry + soonest-expiry eviction; TinyLFU: warm-up + defensive miss + existing-key bypass; SizedCache: payload weighing + heterogeneous sizes + oversized rejection + accessor examples).
- `docs/API.md` Concurrency section corrected — sharded for entry-bounded caches (≥ 32 entries), single-`Mutex` for `SizedCache` regardless of size. Old "0.5 line" / "land in 0.6.0" copy removed.
- Per-cache rustdoc cleanup: stale "0.6.0 implementation", "0.7.0 implementation", "lock-strategy upgrade lands in 0.7.0" headers replaced with a plain "Implementation" heading. Implementation detail still documented; just no more dangling version pins.
- `src/sized.rs` rustdoc explicitly notes that `SizedCache` is intentionally unsharded and explains why.
- `Cargo.toml`: version `0.9.0` → `1.0.0`.

### Verified

- All 9 unit + 47 integration + 17 property + 18 doctests pass. **91 tests total.**
- `cargo fmt --all -- --check` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo clippy --all-targets --no-default-features -- -D warnings` clean.
- `cargo doc --no-deps --all-features` with `RUSTDOCFLAGS="-D warnings"` clean.
- REPS lint surface in `src/lib.rs` honored: no `unsafe`, no `unwrap`, no `expect`, no `print_stdout` / `print_stderr` / `dbg!`, no `todo!` / `unimplemented!`.

---

## [0.9.0] - 2026-05-21

Hardening + audit milestone. No new features. The 0.7.0 surface is locked down with explicit contracts, expanded test coverage, and a license / advisory gate. Skipped 0.8.0 — the `SizedCache` sharding redesign hasn't surfaced as a real-world bottleneck and forcing it through artificially would have been busywork.

### Added

- `#[must_use]` on every `Cache` trait method that returns a value (`get`, `insert`, `remove`, `contains_key`, `len`, `is_empty`, `capacity`). Silently dropping these is almost always a bug — explicit `let _ = ...` is now required to opt out.
- Crate-level **Guarantees** section in `src/lib.rs` rustdoc covering: no `unsafe`, no `panic!` / `unwrap` / `expect` on shipping paths, no background threads, no required runtime, `Send + Sync` everywhere when `K, V` are.
- **9 new unit tests** in `src/sharding.rs` covering the `shard_count` heuristic, `per_shard_capacity` floor-division behaviour, deterministic shard routing, and key distribution across shards.
- **8 new property tests** in `tests/properties.rs`: 4 concurrent-safety properties (4 threads × 256 random ops each, asserting capacity invariant survives interleaving) and 4 cross-cache remove-then-contains symmetry properties.
- `deny.toml` — `cargo deny` configuration locking in license allow-list (MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib, Apache-2.0 WITH LLVM-exception), denying yanked crates, warning on duplicate versions, denying wildcard / unknown-registry / unknown-git dependencies. Run with `cargo deny check all`.

### Changed

- `Cache::insert` rustdoc clarified: the return value carries useful information (new-vs-replace, or admit-vs-reject for `TinyLfuCache`), and the doc now points users at `let _ = ...` if they genuinely want to drop it.
- `Cache::len` rustdoc explicitly notes that the sharded implementation sums per-shard lengths under brief locks — it is **not** an atomic snapshot across shards.
- `Cache::clear` rustdoc explicitly notes that auxiliary state (LFU's priority index, TinyLFU's sketch, the monotonic clocks) is reset alongside the entries.
- `Cache::contains_key` rustdoc clarifies the `TtlCache`-specific nuance: an expired-but-not-yet-cleaned entry is removed during the check.
- Crate-level rustdoc Status section updated — no more stale "rewrites land in 0.6.0" claims. 0.6.0 (arena-backed) and 0.7.0 (sharded) have both shipped.
- `Cargo.toml`: version `0.7.0` → `0.9.0`. Skipped 0.8.0.

### Verified

- All 9 unit + 47 integration + 17 property + 18 doctests pass — **91 tests** total, up from 74.
- `cargo fmt --all -- --check` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo clippy --all-targets --no-default-features -- -D warnings` clean.
- `cargo doc --no-deps --all-features` with `RUSTDOCFLAGS="-D warnings"` clean — zero rustdoc warnings.
- REPS lint surface in `src/lib.rs` honored: every `deny(...)` clippy / rustc lint still applies. No `unsafe` in the crate.

---

## [0.7.0] - 2026-05-20

Concurrency milestone. The single `Mutex<Inner>` from 0.6.x is replaced by a sharded structure across every cache type. Public API is byte-identical — same `Cache` trait, same five cache types, same constructors and signatures. Behavioral contract gains one explicitly-documented approximation: eviction is now **per-shard approximate** rather than strictly global once a cache holds more than a handful of entries.

### Added

- Internal `src/sharding.rs` module with a `Sharded<T>` helper, shard-count heuristic, and per-shard capacity calculator. All five cache types now route operations through `Sharded::shard_for(&key)`.

### Changed

- `LruCache`, `LfuCache`, `TtlCache`, `TinyLfuCache`: internal `Mutex<Inner>` replaced by `Sharded<Inner>` — up to 16 independent shards routed by `DefaultHasher`-based key hash. Each shard holds its own arena / `HashMap` / priority index / sketch. Lock contention is bounded by per-shard traffic, not by total cache traffic.
- `SizedCache` deliberately **stays unsharded** in 0.7.0. Splitting `max_weight` evenly across shards produces a per-shard weight ceiling small enough to reject values that would have fit comfortably in the unsharded cache. A future release can revisit with a smarter routing scheme (e.g. shard purely for the lookup `HashMap` while keeping a single global weight budget); for 0.7.0 the safer call is to keep the 0.6.0 single-`Mutex` implementation intact.
- Shard-count heuristic: tiny caches (capacity below `MIN_PER_SHARD * 2` = 32 entries, or `max_weight < 32` for `SizedCache`) use a single shard and retain strict global eviction ordering. Larger caches scale up to 16 shards while keeping per-shard size at or above `MIN_PER_SHARD` (16). This preserves the existing smoke tests' deterministic behaviour at small capacities while giving production-sized caches the concurrency win.
- Per-shard capacity is computed by floor division (`total / num_shards`). Total effective capacity may be at most `num_shards - 1` less than requested when capacity isn't divisible by shard count. Documented on each cache type.
- `TinyLfuCache`: Count-Min Sketch is now **per-shard**, not global. A global sketch would force every access through a shared structure and defeat the point of sharding. Per-shard sketches still capture the local frequency signal accurately, which is what the per-shard admission decision needs.
- Type-level documentation on every cache surfaces the per-shard-approximate-eviction caveat explicitly.
- `Cargo.toml`: version `0.6.0` → `0.7.0`.

### Verified

- All 47 integration tests, 9 property tests, and 18 doctests pass unchanged. The single-shard-for-tiny-caches heuristic intentionally preserves the strict-ordering expectations of the existing test suite.
- `cargo bench` compiles cleanly. Multi-threaded contention numbers are workload-dependent; running `cargo bench` after a fresh build will surface the local improvement.

---

## [0.6.0] - 2026-05-20

Implementation-quality milestone. Public surface is byte-identical to 0.5.x — every existing call-site compiles and behaves identically — but the internal data structures behind every cache type changed. Asymptotic complexity is better across the board.

### Changed

- `LruCache`: replaced the `Mutex<{ HashMap<K, V>, VecDeque<K> }>` reference implementation with a `Mutex<{ Vec<Option<Node>>, free-list, head/tail indices, HashMap<K, usize> }>` arena. `get` and `insert` now do O(1) promotes; eviction is O(1). The 0.5.x `VecDeque::iter().position()` scan on every access is gone.
- `TinyLfuCache`: arena-backed in the same shape as `LruCache`. The Count-Min Sketch, admission filter, and aging step are unchanged. O(1) promote / O(1) admission decision (was O(n) victim scan).
- `SizedCache`: arena-backed with weight bookkeeping in each node. O(1) promote, O(1) per eviction step (a single insert may loop the eviction step until the weight invariant is restored). The 0.5.x `VecDeque::iter().position()` scan is gone.
- `LfuCache`: replaced the O(n) victim scan with a `BTreeMap<(count, age), K>` priority index. Every access and eviction is now O(log n). The trade-off is one extra `K::clone()` per access (the priority index needs to know the key) — paid back many times over once the cache holds more than a few dozen entries.
- `TtlCache`: unchanged. The lazy-expiry pattern already pays for itself, and the typical TTL access pattern (read-once, evict-on-access) does not benefit from an arena. Will be revisited in a later release if profiling shows a hot spot.
- Lock strategy unchanged across all types: still a single `Mutex<Inner>`. Sharded `Mutex` (DashMap-style) or `crossbeam-epoch` lock-free reclamation deferred to **0.7.0** — a separate concurrency-focused release.
- `Cargo.toml`: version `0.5.1` → `0.6.0`.

### Verified

- All 47 integration tests, 9 property tests, and 18 doctests pass unchanged.
- `cargo bench` compiles cleanly. Workload-dependent timings are intentionally not reproduced in the changelog — run locally for accurate baselines.

---

## [0.5.1] - 2026-05-20

Docs and repo hygiene. Library code is byte-identical to 0.5.0 — `cargo update` reads no new public symbols. The user-visible improvements are concentrated on the crates.io / GitHub side: cleaner README, proper API reference, public release archive, and an MSRV CI fix that landed without a release marker.

### Added

- `docs/API.md` — complete public API reference with signatures, contracts, and code examples for every public item.
- `docs/README.md` — documentation index with release archive table and quick links.
- `docs/release/` — public archive of per-version release notes (v0.1.0 → v0.5.0). Each file is public-safe and matches what is posted to GitHub Releases.

### Changed

- README: removed the stale `**Edition:** 2024.` line (the crate is on edition 2021) and added a "Documentation" section linking to `docs/API.md`, `docs/README.md`, `CHANGELOG.md`, `REPS.md`, and `docs.rs`.
- `.gitignore` now ignores the private internal working directory. Earlier rules only covered scratch and temp subpaths, so internal planning and release-working files were still tracked. Public release notes remain under `docs/release/`.
- Untracked the historical internal working directory so the 0.5.1 commit removed those files from the GitHub-visible tree at HEAD. Files remain in past commit history; a full history scrub is a separate `git filter-repo` operation if ever needed.

### Fixed

- CI: MSRV row (Rust 1.75) now runs `cargo check --lib --all-features` instead of the full clippy/test/doc sweep. Dev-dependencies (criterion 0.5 → clap_builder 4.6) transitively require `edition2024` (Rust 1.85+), which the MSRV toolchain can't parse. The library itself still builds cleanly on 1.75; the build-tool MSRVs are independent of the library's MSRV promise. Same pattern config-lib uses.

### Security

---

## [0.5.0] - 2026-05-20

### Added

- Public `TinyLfuCache<K, V>` — a Count-Min Sketch frequency estimator (depth 4, `u8` saturating counters, periodic halving) plus an admission filter on top of an LRU-ordered main cache. On capacity overflow, a new key is **admitted only if its estimated frequency exceeds the LRU victim's**; otherwise the insert call returns `None` and the value is dropped. This is a deliberate semantic deviation from the other cache types — surfaced prominently in the type docs.
- Public `SizedCache<K, V>` — capacity bound is **total byte-weight**, not entry count. A user-supplied `fn(&V) -> usize` weighs each value at insert time; LRU eviction makes room when an insert would overshoot `max_weight`. Exposes `SizedCache::max_weight()` and `SizedCache::total_weight()` as the meaningful queries; `Cache::capacity()` returns `max_weight` for consistency. Values whose own weight exceeds `max_weight` are silently rejected (the only sane response).
- Property tests via `proptest` — five capacity-invariant properties (`len <= capacity` / `total_weight <= max_weight` after arbitrary op sequences) plus four insert-then-get round-trip properties. Run on every CI invocation.
- Criterion benchmarks — five-group suite (`LruCache` / `LfuCache` / `TtlCache` / `TinyLfuCache` / `SizedCache`) covering `get_hit` and `insert_existing` at capacity 1024. Gated behind `required-features = ["std"]` so `--no-default-features` builds still pass.
- 15 new integration tests: 6 covering `TinyLfuCache` admission semantics and 9 covering `SizedCache` byte-weight bookkeeping. Total integration test count: 47.
- 5 new doctests on the new types and constructors. Total doctest count: 18.

### Changed

- Crate-level docs in `src/lib.rs` now describe the public surface as "feature-complete" — five cache types live behind the [`Cache`] trait. The `LruCache` / `LfuCache` / `TtlCache` / `TinyLfuCache` docs no longer claim a 0.5.0 lock-free upgrade; that work is deferred to a later minor with no API-surface impact.
- `Cache::capacity` rustdoc generalized to acknowledge `SizedCache`'s byte-weight interpretation alongside the entry-count interpretation used by the other policies.
- Internal: extracted the `pub(crate) trait MutexExt::lock_recover` into `src/util.rs` and refactored `LruCache` / `LfuCache` / `TtlCache` to use it. Three identical poison-recovery helpers reduced to one. `find_victim`-style scan helpers stay per-policy because their comparison criteria differ.
- `Cargo.toml`: version `0.4.0` → `0.5.0`. Added `proptest` and `criterion` as dev-dependencies. Declared the `[[bench]]` target.

### Fixed

### Security

---

## [0.4.0] - 2026-05-20

### Added

- Public `TtlCache<K, V>` — bounded, thread-safe cache with per-entry time-to-live and lazy expiry. Each entry stamped with a deadline at insert time; expired entries are removed lazily on `get` / `contains_key` / `len`. On overflow the entry with the **soonest expiration** is evicted, which naturally prefers already-expired entries over live ones.
- `TtlCache::insert_with_ttl(&self, key, value, ttl)` — per-call TTL override that ignores the cache default for that one entry.
- Constructors `TtlCache::new(usize, Duration) -> Result<Self, CacheError>` and `TtlCache::with_capacity(NonZeroUsize, Duration) -> Self` mirroring `LruCache` / `LfuCache`.
- 13 integration tests covering: zero-capacity rejection, in-window get, lazy expiry through `get` / `contains_key` / `len`, per-call TTL override, soonest-expiry-first eviction, preference for already-expired entries over live ones, stale-as-absent on overwrite, live-update returning the old value, removal, clear, and `Send + Sync`. 4 new doctests on the type + both constructors + `insert_with_ttl`.

### Changed

- Crate-level docs in `src/lib.rs` updated to list `LruCache`, `LfuCache`, and `TtlCache` as the shipped reference implementations; trajectory clarified to point TinyLFU and `SizedCache` at 0.5.0.
- `Cargo.toml`: version `0.3.0` → `0.4.0`.

### Fixed

### Security

---

## [0.3.0] - 2026-05-20

### Added

- Public `LfuCache<K, V>` — bounded, thread-safe Least-Frequently-Used cache. Each entry carries a counter that increments on every `get` / `insert` of an already-present key. On overflow, the entry with the lowest counter is evicted; ties are broken in favour of the least-recently-accessed entry. Same `&self`-everywhere shape as `LruCache`: `Send + Sync`, poison-tolerant `Mutex`, no new dependencies.
- Constructors `LfuCache::new(usize) -> Result<Self, CacheError>` and `LfuCache::with_capacity(NonZeroUsize) -> Self` mirroring `LruCache`.
- Eight integration tests covering counter increments, LRU tie-break, `contains_key` non-promotion, replacement, removal, clear, capacity preservation, and `Send + Sync`.

### Changed

- Crate-level docs in `src/lib.rs` updated to list `LruCache` and `LfuCache` as the shipped reference implementations.
- `Cargo.toml`: version `0.2.0` → `0.3.0`.

### Fixed

### Security

---

## [0.2.0] - 2026-05-20

### Added

- Public `Cache<K, V>` trait — the common contract every cache type in this crate implements. Operations: `get`, `insert`, `remove`, `contains_key`, `len`, `is_empty`, `clear`, `capacity`. All methods take `&self` so instances are usable across threads and `.await` points without external locking.
- Public `LruCache<K, V>` — reference Least-Recently-Used implementation backed by a `Mutex<{ HashMap, VecDeque }>`. Bounded capacity; inserts past capacity evict the least-recently-accessed entry. `get` and `insert` promote to MRU; `contains_key` deliberately does not (query-only).
- Public `CacheError` enum (`#[non_exhaustive]`) with `InvalidCapacity` variant. `std::error::Error` impl gated on the `std` feature.
- Constructors `LruCache::new(usize) -> Result<Self, CacheError>` and `LruCache::with_capacity(NonZeroUsize) -> Self` (infallible).
- Integration tests covering insert/get, replacement, LRU eviction order, `contains_key` non-promotion, removal, clear, capacity reporting, `Send + Sync`, and error display.

### Changed

- `Cargo.toml`: version `0.1.0` → `0.2.0`; `edition` lowered from `2024` to `2021` so the declared MSRV of Rust 1.75 is actually buildable (edition 2024 requires Rust 1.85+). Scaffold-stage oversight surfaced by the 0.1.0 CI run.
- CI: removed the unused `actions/setup-node@v5` step (cache-mod is a pure Rust crate; Node was only present for the JS-action runtime).
- CI: switched dependency caching from `actions/cache@v4` to `Swatinem/rust-cache@v2`, aligning with the rest of the crate family and yielding Rust-aware cache keys.
- `rustfmt.toml`: `edition` synced to `2021` to match `Cargo.toml`.
- Added `.gitattributes` enforcing `eol=lf` across the tree so Windows CI runners (which default to `core.autocrlf=true`) don't trip rustfmt's `newline_style = "Unix"` setting.
- Stripped UTF-8 BOMs from every scaffold-generated text file (`Cargo.toml`, `README.md`, `CHANGELOG.md`, `REPS.md`, `clippy.toml`, `rustfmt.toml`, `.gitignore`, `.editorconfig`). Cargo tolerates a leading BOM, but the `Swatinem/rust-cache@v2` TOML parser does not — it was logging `Error parsing Cargo.toml manifest, fallback to caching entire file` on every run and losing per-package cache scoping.

### Fixed

- CI: resolved the baseline failures from the 0.1.0 scaffold run (manifest parse error on the MSRV row, "Incorrect newline style" on Windows, Node 20 deprecation warning on the cache action). Fix was deferred from 0.1.0 per release policy.

### Security

- CI: added `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true` at the workflow level to opt every JavaScript action into the Node.js 24 runtime ahead of the June 2, 2026 forced migration.

---

## [0.1.0] - 2026-05-18

### Added

- Initial scaffold and repository bootstrap.
- REPS compliance baseline.
- CI for Linux/macOS/Windows on stable and MSRV (1.75).

[Unreleased]: https://github.com/jamesgober/cache-mod/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jamesgober/cache-mod/releases/tag/v1.0.0
[0.9.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.9.0
[0.7.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.7.0
[0.6.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.6.0
[0.5.1]: https://github.com/jamesgober/cache-mod/releases/tag/v0.5.1
[0.5.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.5.0
[0.4.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.4.0
[0.3.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.3.0
[0.2.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.2.0
[0.1.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.1.0
