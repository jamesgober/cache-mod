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

## [0.5.1] - 2026-05-20

Docs and repo hygiene. Library code is byte-identical to 0.5.0 — `cargo update` reads no new public symbols. The user-visible improvements are concentrated on the crates.io / GitHub side: cleaner README, proper API reference, public release archive, and an MSRV CI fix that landed without a release marker.

### Added

- `docs/API.md` — complete public API reference with signatures, contracts, and code examples for every public item.
- `docs/README.md` — documentation index with release archive table and quick links.
- `docs/release/` — public archive of per-version release notes (v0.1.0 → v0.5.0). Each file is public-safe and matches what is posted to GitHub Releases.

### Changed

- README: removed the stale `**Edition:** 2024.` line (the crate is on edition 2021) and added a "Documentation" section linking to `docs/API.md`, `docs/README.md`, `CHANGELOG.md`, `REPS.md`, and `docs.rs`.
- `.gitignore` now ignores the entire `.dev/` folder. Earlier rules only ignored `.dev/scratch/` and `.dev/tmp/`; `.dev/PROMPT.md`, `.dev/DIRECTIVES.md`, `.dev/ROADMAP.md`, and `.dev/release/` were tracked. `.dev/` is private working state — release notes that should be public now live at `docs/release/`.
- Untracked the entire historical `.dev/` directory tree (`git rm -r --cached .dev/`) so the commit that ships 0.5.1 removes those files from the GitHub-visible tree. Files remain in past commit history; a full history scrub is a separate `git filter-repo` operation if ever needed.

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

[Unreleased]: https://github.com/jamesgober/cache-mod/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/jamesgober/cache-mod/releases/tag/v0.5.1
[0.5.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.5.0
[0.4.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.4.0
[0.3.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.3.0
[0.2.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.2.0
[0.1.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.1.0
