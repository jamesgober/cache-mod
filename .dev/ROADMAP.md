# cache-mod - Roadmap to 1.0

Fast-track. No slow-stepping.

---

## Release process (every versioned main update)

- [ ] Version updated in `Cargo.toml`
- [ ] Version notes updated in `CHANGELOG.md`
- [ ] Documentation updated for API and behavior accuracy
- [ ] Private release log written to `.dev/release/v<version>.md`
- [ ] Release log is public-safe (no `.dev/` links/references)

---

## Phase 0.1.0 - Scaffold (done)

- [x] Repository created
- [x] Cargo.toml, README, LICENSE x2, CHANGELOG
- [x] REPS.md
- [x] CI workflow (Linux/macOS/Windows on stable + MSRV, Node 24)
- [x] Initial commit pushed

---

## Phase 0.2.0 - Foundation

Define the public API surface.

Skill areas in scope:

  - cache eviction algorithms
  - TinyLFU implementation
  - lock-free design
  - typed APIs

- [x] Public types defined
- [x] Public traits defined
- [x] Module structure laid out
- [x] Error type defined
- [x] First end-to-end smoke test passing
- [x] Resolve baseline CI failures from run 26157634322 (carry-forward from 0.1.0)
- [x] CHANGELOG updated
- [x] `.dev/release/v0.2.0.md` written

---

## Phase 0.3.0 - LFU eviction policy (done)

- [x] `LfuCache<K, V>` reference implementation (Mutex-guarded, O(n) eviction scan)
- [x] Eviction semantics: minimum counter, ties broken by least-recently-accessed
- [x] `contains_key` honors Cache-trait non-promotion contract
- [x] Integration tests covering counter behavior and tie-break
- [x] Doctest examples on type + constructors
- [x] CHANGELOG updated
- [x] README updated to reflect shipped policies
- [x] `.dev/release/v0.3.0.md` written

---

## Phase 0.4.0 - TTL eviction policy (done)

- [x] `TtlCache<K, V>` reference implementation with lazy expiry on access
- [x] Per-entry expiration; bounded capacity for hard ceiling
- [x] Optional per-call TTL override (`insert_with_ttl`)
- [x] Soonest-expiry-first eviction (prefers already-expired over live entries)
- [x] `Instant::checked_add` overflow guard with far-future fallback
- [x] Integration tests using short TTLs + small sleeps (no clock injection yet)
- [x] CHANGELOG updated
- [x] README updated
- [x] `.dev/release/v0.4.0.md` written

---

## Phase 0.5.0 - TinyLFU + implementation quality

- [ ] `TinyLfuCache<K, V>` — admission filter (Count-Min Sketch) + LFU main
- [ ] Lock-free arena-backed rewrite of `LruCache` and `LfuCache` internals (public API unchanged)
- [ ] `SizedCache<K, V>` — byte-bound capacity policy (composes with primary policies)
- [ ] All public API methods implemented (no `todo!()`)
- [ ] Property tests for state machines / invariants
- [ ] Basic benchmarks (Criterion)
- [ ] No `unwrap` / `expect` outside of tests
- [ ] CHANGELOG updated
- [ ] `.dev/release/v0.5.0.md` written

---

## Phase 0.9.0 - Hardening + Audit

Feature freeze. Quality focus.

### Audit checklist (mandatory)

#### Feature completeness
- [ ] Every roadmap item delivered
- [ ] Every README claim verified

#### Code cleanliness
- [ ] No dead code
- [ ] No commented-out code
- [ ] No TODO/FIXME without tracking issue
- [ ] No `#[allow(...)]` without justification

#### Error hardening
- [ ] Every public function: all error paths documented
- [ ] Every error variant: documented + tested
- [ ] No panics in shipping code paths
- [ ] Error messages actionable

#### API stability
- [ ] Every public item reviewed for 1.0
- [ ] Sealed traits where appropriate
- [ ] `#[non_exhaustive]` on growth-likely enums

#### Documentation
- [ ] Every public item: rustdoc with example
- [ ] README accurate
- [ ] CHANGELOG complete
- [ ] `cargo doc` zero warnings

#### Tests
- [ ] Unit test coverage on all public functions
- [ ] Integration tests
- [ ] Property tests for invariants
- [ ] Cross-platform CI green
- [ ] Both stable and MSRV green

#### Performance
- [ ] Hot paths benchmarked
- [ ] Allocation profile checked
- [ ] No regressions
- [ ] Benchmark baselines saved

#### Final
- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean
- [ ] `cargo test --all-features` clean
- [ ] `cargo doc` clean with `RUSTDOCFLAGS=-D warnings`

### Output
- [ ] `.dev/release/v0.9.0.md` written
- [ ] Audit findings logged
- [ ] All findings resolved or deferred to 1.x

---

## Phase 0.9.x - Audit fixes

- [ ] All 0.9.0 blockers resolved
- [ ] No new features
- [ ] Final benchmarks recorded
- [ ] Final API freeze

---

## Phase 1.0.0 - Stable release

- [ ] All 0.9.x findings resolved
- [ ] Final API freeze
- [ ] Final benchmarks captured
- [ ] `.dev/release/v1.0.0.md` written
- [ ] Tag `v1.0.0` on main
- [ ] Publish to crates.io