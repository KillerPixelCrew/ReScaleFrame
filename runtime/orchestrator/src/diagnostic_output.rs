// SPDX-License-Identifier: MIT
use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

// Serialize borrowed sink callbacks and let owners drain output before release.
static OUTPUT: Mutex<()> = Mutex::new(());
thread_local! { static IN_OUTPUT: Cell<bool> = const { Cell::new(false) }; }

pub(super) struct Output {
    _guard: MutexGuard<'static, ()>,
}

impl Output {
    pub(super) fn active_here() -> bool {
        IN_OUTPUT.get()
    }

    pub(super) fn enter() -> Option<Self> {
        if Self::active_here() {
            return None;
        }
        let guard = OUTPUT.lock().unwrap_or_else(PoisonError::into_inner);
        IN_OUTPUT.set(true);
        Some(Self { _guard: guard })
    }

    pub(super) fn try_enter() -> Option<Self> {
        if Self::active_here() {
            return None;
        }
        let guard = match OUTPUT.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        IN_OUTPUT.set(true);
        Some(Self { _guard: guard })
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        IN_OUTPUT.set(false);
    }
}
