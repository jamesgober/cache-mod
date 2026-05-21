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

## What it does

High-performance in-process caching with multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No dependency on any external store.

---

## Quick start

```toml
[dependencies]
cache-mod = "1"
```

```rust
use std::time::Duration;
use cache_mod::{Cache, LfuCache, LruCache, SizedCache, TinyLfuCache, TtlCache};

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
ttl.insert_with_ttl("flash", 7, Duration::from_secs(5));

// TinyLFU — Count-Min Sketch admission filter rejects cold candidates.
let tinylfu: TinyLfuCache<&'static str, u32> = TinyLfuCache::new(64).expect("capacity > 0");
tinylfu.insert("hot", 1);

// SizedCache — capacity is total byte-weight, not entry count.
fn byte_weight(v: &Vec<u8>) -> usize { v.len() }
let sized: SizedCache<&'static str, Vec<u8>> =
    SizedCache::new(4 * 1024, byte_weight).expect("max_weight > 0");
sized.insert("payload", vec![0u8; 256]);
```

### FEATURES

- `Cache<K, V>` trait — the common read / write / evict contract.
- `LruCache<K, V>` — bounded, thread-safe Least-Recently-Used cache.
- `LfuCache<K, V>` — bounded, thread-safe Least-Frequently-Used cache.
- `TtlCache<K, V>` — bounded, thread-safe Time-To-Live cache with lazy expiry.
- `TinyLfuCache<K, V>` — Count-Min Sketch admission filter + LRU main cache.
- `SizedCache<K, V>` — capacity bound is total byte-weight across entries.
- `CacheError` — error type returned by constructors.

Arena-backed internals (O(1) for LRU/TinyLFU/Sized, O(log n) for LFU)
and sharded concurrency (up to 16 shards for entry-bounded caches) ship
under the same public surface. Future internal improvements within the
1.x line will be source-compatible.

---

## Documentation

- **[API Reference](docs/API.md)** — every public item, with signature, contract, and code examples.
- **[Stability promise](docs/STABILITY.md)** — frozen 1.0 surface + SemVer commitments.
- **[Docs index](docs/README.md)** — release archive + quick links.
- **[CHANGELOG](CHANGELOG.md)** — per-version diff log.
- **[REPS standards](REPS.md)** — quality discipline this crate is held to.
- Machine-rendered rustdoc: **[docs.rs/cache-mod](https://docs.rs/cache-mod)**.

---

## Standards

- **REPS** governs every decision. See [REPS.md](REPS.md).
- **MSRV:** Rust 1.75.
- **Edition:** 2021.
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