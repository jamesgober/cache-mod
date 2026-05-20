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

**Active development.** Foundation milestone (0.2.0) shipped. On the path to 1.0.

The public API is not yet frozen. Pin specific versions; expect additive (and occasionally breaking) changes pre-1.0.

---

## What it does

High-performance in-process caching with multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No dependency on any external store.

---

## Quick start

```toml
[dependencies]
cache-mod = "0.4"
```

```rust
use std::time::Duration;
use cache_mod::{Cache, LfuCache, LruCache, TtlCache};

// LRU — evicts the least-recently-accessed entry on overflow.
let lru: LruCache<&'static str, u32> = LruCache::new(64).expect("capacity > 0");
lru.insert("requests", 1);
assert_eq!(lru.get(&"requests"), Some(1));

// LFU — evicts the lowest-counter entry on overflow.
let lfu: LfuCache<&'static str, u32> = LfuCache::new(64).expect("capacity > 0");
lfu.insert("requests", 1);

// TTL — entries expire after their per-entry deadline; lazy expiry on access.
let ttl: TtlCache<&'static str, u32> =
    TtlCache::new(64, Duration::from_secs(300)).expect("capacity > 0");
ttl.insert("session", 42);
ttl.insert_with_ttl("flash", 7, Duration::from_secs(5));
```

### What's shipped

- `Cache<K, V>` trait — the common read / write / evict contract.
- `LruCache<K, V>` — bounded, thread-safe Least-Recently-Used cache.
- `LfuCache<K, V>` — bounded, thread-safe Least-Frequently-Used cache.
- `TtlCache<K, V>` — bounded, thread-safe Time-To-Live cache with lazy expiry.
- `CacheError` — error type returned by constructors.

TinyLFU and size-bounded variants land in 0.5.0 alongside lock-free,
arena-backed rewrites of the existing reference implementations — public
surface unchanged.

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