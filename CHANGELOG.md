# Changelog

All notable changes to `cache-mod` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

### Changed

- `Cargo.toml`: `edition` lowered from `2024` to `2021` so the declared MSRV of Rust 1.75 is actually buildable. Edition 2024 requires Rust 1.85+; mismatch was a scaffold-stage oversight surfaced by the 0.1.0 CI run.
- CI: removed the unused `actions/setup-node@v5` step (cache-mod is a pure Rust crate; Node was only present for the JS-action runtime).
- CI: switched dependency caching from `actions/cache@v4` to `Swatinem/rust-cache@v2`, aligning with the rest of the crate family and yielding Rust-aware cache keys.

### Fixed

- CI: resolved the baseline failures from the 0.1.0 scaffold run (manifest parse error on the MSRV row and Node 20 deprecation warning on the cache action). The fix was deferred from 0.1.0 per release policy.

### Security

- CI: added `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true` at the workflow level to opt every JavaScript action into the Node.js 24 runtime ahead of the June 2, 2026 forced migration.

---

## [0.1.0] - 2026-05-18

### Added

- Initial scaffold and repository bootstrap.
- REPS compliance baseline.
- CI for Linux/macOS/Windows on stable and MSRV (1.75).

[Unreleased]: https://github.com/jamesgober/cache-mod/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jamesgober/cache-mod/releases/tag/v0.1.0