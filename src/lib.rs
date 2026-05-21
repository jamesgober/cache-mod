//! # cache-mod
//!
//! HIGH-PERFORMANCE IN-PROCESS CACHING
//!
//! Multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe,
//! lock-minimized internals. Typed key-value API. No external store dependency.
//!
//! # Status
//!
//! The public API surface is feature-complete: the [`Cache`] trait, the
//! [`CacheError`] error type, and five reference cache implementations —
//! [`LruCache`] (Least-Recently-Used), [`LfuCache`] (Least-Frequently-Used),
//! [`TtlCache`] (Time-To-Live, lazy expiry), [`TinyLfuCache`] (Count-Min Sketch
//! admission filter + LRU main), and [`SizedCache`] (byte-bound capacity).
//! Internals are arena-backed (0.6.0) and sharded for concurrent throughput
//! (0.7.0). The API is currently in the 0.9.x hardening cycle; it is **not**
//! yet frozen, but no further breaking changes are planned before the 1.0
//! release. Pin exact versions until 1.0 is tagged.
//!
//! # Guarantees
//!
//! - **No `unsafe`.** This crate contains zero `unsafe` blocks. Every cache
//!   operation goes through safe abstractions only.
//! - **No `panic!`, no `unwrap`, no `expect`.** The library code never calls
//!   any of these macros / methods on shipping paths. Where a non-panicking
//!   `unreachable!()` appears, it documents an invariant the caller is
//!   responsible for upholding inside `pub(crate)` arena helpers; every
//!   public method is panic-free given valid arguments. `clippy::unwrap_used`
//!   and `clippy::expect_used` are denied at the crate level.
//! - **No background threads.** Every cache type uses lazy bookkeeping —
//!   eviction, expiry, and sketch aging run on the call-site thread that
//!   triggered them. No `std::thread::spawn` happens inside the crate.
//! - **No required runtime.** The crate has no async runtime dependency.
//!   `&self` everywhere lets you share cache instances across `.await`
//!   points if you do use one.
//! - **Send + Sync.** Every cache type is `Send + Sync` when `K: Send + Sync`
//!   and `V: Send + Sync`.
//!
//! # Quick start
//!
//! ```
//! use cache_mod::{Cache, LruCache};
//!
//! let cache: LruCache<&'static str, u32> = LruCache::new(64).expect("capacity > 0");
//!
//! cache.insert("requests", 1);
//! cache.insert("errors", 0);
//!
//! assert_eq!(cache.get(&"requests"), Some(1));
//! assert_eq!(cache.len(), 2);
//! ```
//!
//! # License
//!
//! Dual-licensed under Apache-2.0 OR MIT.

#![doc(html_root_url = "https://docs.rs/cache-mod")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::missing_safety_doc)]

mod cache;
mod error;

#[cfg(feature = "std")]
mod lfu;
#[cfg(feature = "std")]
mod lru;
#[cfg(feature = "std")]
mod sharding;
#[cfg(feature = "std")]
mod sized;
#[cfg(feature = "std")]
mod tinylfu;
#[cfg(feature = "std")]
mod ttl;
#[cfg(feature = "std")]
mod util;

pub use cache::Cache;
pub use error::CacheError;

#[cfg(feature = "std")]
pub use lfu::LfuCache;
#[cfg(feature = "std")]
pub use lru::LruCache;
#[cfg(feature = "std")]
pub use sized::SizedCache;
#[cfg(feature = "std")]
pub use tinylfu::TinyLfuCache;
#[cfg(feature = "std")]
pub use ttl::TtlCache;

/// Crate version string, populated by Cargo at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
