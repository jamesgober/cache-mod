//! # cache-mod
//!
//! HIGH-PERFORMANCE IN-PROCESS CACHING
//!
//! Multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe,
//! lock-minimized internals. Typed key-value API. No external store dependency.
//!
//! # Status
//!
//! Foundation milestone (0.2.0). The public API surface is defined: the
//! [`Cache`] trait, the [`LruCache`] reference implementation, and the
//! [`CacheError`] error type. LFU, TinyLFU, TTL, and size-bounded eviction
//! policies land in subsequent minors. The API is not yet frozen — pin
//! exact versions until 1.0.
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
mod lru;

pub use cache::Cache;
pub use error::CacheError;

#[cfg(feature = "std")]
pub use lru::LruCache;

/// Crate version string, populated by Cargo at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
