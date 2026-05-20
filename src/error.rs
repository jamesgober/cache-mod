//! Error type returned by `cache-mod` constructors and operations.

use core::fmt;

/// Errors produced by `cache-mod`.
///
/// The enum is `#[non_exhaustive]` — new variants may be added in minor
/// releases as additional cache types and eviction policies land.
///
/// # Example
///
/// ```
/// use cache_mod::{CacheError, LruCache};
///
/// // Zero capacity is rejected up-front.
/// let result = LruCache::<u32, u32>::new(0);
/// assert_eq!(result.err(), Some(CacheError::InvalidCapacity));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheError {
    /// A cache constructor was called with a capacity of zero.
    ///
    /// Every eviction policy in this crate requires at least one entry of
    /// headroom, so capacity must be `>= 1`.
    InvalidCapacity,
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCapacity => f.write_str("cache capacity must be non-zero"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CacheError {}
