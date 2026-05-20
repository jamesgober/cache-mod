<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <strong>cache-mod</strong>
    <br>
    <sup><sub>HIGH-PERFORMANCE IN-PROCESS CACHING</sub></sup>
</h1>

<p align="center">
    <a href="https://crates.io/crates/cache-mod"><img alt="crates.io" src="https://img.shields.io/crates/v/cache-mod.svg"></a>
    <a href="https://crates.io/crates/cache-mod"><img alt="downloads" src="https://img.shields.io/crates/d/cache-mod.svg?color=0099ff"></a>
    <a href="https://docs.rs/cache-mod"><img alt="docs.rs" src="https://docs.rs/cache-mod/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md" title="MSRV"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.75%2B-blue"></a>
    <a href="https://github.com/jamesgober/cache-mod/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/jamesgober/cache-mod/actions/workflows/ci.yml/badge.svg"></a>
</p>

<p align="center">Multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No external store dependency.</p>

---

## Status

**Active development.** Scaffolded and on the path to 1.0.

The public API is not yet stable. Pin specific versions; expect changes pre-1.0.

---

## What it does

High-performance in-process caching with multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No dependency on any external store.

---

## Quick start

```toml
[dependencies]
cache-mod = "0.1"
```

---

## Standards

- **REPS** governs every decision. See [REPS.md](REPS.md).
- **MSRV:** Rust 1.75.
- **Edition:** 2024.
- **Cross-platform:** Linux, macOS, Windows.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

<!-- FOOT COPYRIGHT
################################################# -->
<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sup>
</div>