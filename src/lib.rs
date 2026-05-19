//! # cache-mod
//!
//! HIGH-PERFORMANCE IN-PROCESS CACHING
//!
//! Multiple eviction policies (LRU, LFU, TinyLFU, TTL, size-bounded). Async-safe, lock-minimized internals. Typed key-value API. No external store dependency.
//!
//! # Status
//!
//! Early scaffolding. Public API not yet defined. See [the repository](https://github.com/jamesgober/cache-mod)
//! and .dev/ROADMAP.md for the milestone plan.
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

/// Crate version string, populated by Cargo at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");