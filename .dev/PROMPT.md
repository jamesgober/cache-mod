# cache-mod - Project Prompt

## Priority order

1. `REPS.md` - SUPREME AUTHORITY
2. `.dev/DIRECTIVES.md`
3. This file (`.dev/PROMPT.md`)
4. `.dev/ROADMAP.md`

## What this crate is

High-performance in-process caching with multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No dependency on any external store.

## Why it exists

Multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No external store dependency.

## Skill areas

- cache eviction algorithms
- TinyLFU implementation
- lock-free design
- typed APIs

## Scope (1.0)

Defined in `.dev/ROADMAP.md`.

## Out of scope (always)

- Features requiring async runtime hard-dependency
- Features pulling in heavy framework dependency
- Features that violate REPS

## Pre-1.0 audit (mandatory)

See `.dev/ROADMAP.md` for the audit checklist. Must verify:

- Feature completeness vs. the roadmap
- API accuracy and stability
- Code cleanliness
- Error hardening
- Documentation completeness
- Test coverage
- Benchmark coverage
- Cross-platform CI passing

## Versioning

Fast-track. No slow-stepping:

- 0.1.0 - scaffold
- 0.2.0 - first real implementation
- 0.5.0 - most features in place
- 0.9.0 - feature-complete, hardening
- 0.9.x - audit findings
- 1.0.0 - stable

## Release discipline (mandatory)

For each main update that is ready to push and includes a version change:

- Write a private release log at `.dev/release/v<version>.md`
- Follow the release structure used in metrics-lib style releases (clear summary, categorized changes, verification status)
- Keep release text public-safe: do not include `.dev/` links or references intended for public release notes
- Update versioned artifacts together: `Cargo.toml`, `CHANGELOG.md`, and docs that describe API/behavior

Exception policy:

- Do not backport baseline CI failures into `0.1.0`
- Fix baseline CI failures in the next version (`0.2.0`)