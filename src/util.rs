//! Internal utilities shared across cache implementations.
//!
//! Nothing in this module is part of the public API.

use std::sync::{Mutex, MutexGuard};

/// Extension trait giving every cache a poison-tolerant `lock`.
///
/// A panic inside any operation that holds an inner `Mutex` poisons it.
/// The cache's invariants are not weakened by a poisoned lock — every
/// operation re-establishes consistency before returning — so we recover
/// the guard rather than propagating the poison upward as a user-visible
/// error.
pub(crate) trait MutexExt<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
