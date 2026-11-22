// SPDX-License-Identifier: MIT
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, taking over the data of a thread that panicked while holding it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
