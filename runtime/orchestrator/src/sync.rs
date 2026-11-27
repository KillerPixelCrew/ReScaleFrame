// SPDX-License-Identifier: MIT
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

/// Locks `mutex`, taking over the data of a thread that panicked while holding it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Locks `mutex` unless another owner holds it; a poisoned lock is taken over as in `lock`.
pub(crate) fn try_lock<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}
