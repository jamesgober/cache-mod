# cache-mod - Directives

## Priority

1. `REPS.md` - SUPREME AUTHORITY
2. This file
3. `.dev/PROMPT.md`
4. `.dev/ROADMAP.md`

## Cross-platform

Linux, macOS, Windows. Platform-specific code feature-gated.

## REPS compliance

- Zero-allocation hot path
- Lock-free where contention matters
- `unsafe` only when measured and documented
- No `unwrap()` / `expect()` / `todo!()` / `unimplemented!()`
- No `print_stdout` / `print_stderr` / `dbg!()`
- All public items documented with one example

## Versioning

Fast-track to 1.0. Every release tagged. `.dev/release/v<version>.md` for each.

Release-update gate (required for non-hotfix main updates):

- Version bump is synchronized across `Cargo.toml` and `CHANGELOG.md`
- Documentation is updated for API, behavior, and examples
- A release log is created in `.dev/release/v<version>.md`
- Release log content is public-safe and does not reference `.dev/` paths

CI policy:

- Known baseline CI failures from the scaffold stage are not fixed in `0.1.0`
- Those failures must be fixed in the next version update (`0.2.0`)

## Pre-1.0 audit

Mandatory. Checklist in `.dev/ROADMAP.md`. Report in `.dev/release/v1.0.0.md`.