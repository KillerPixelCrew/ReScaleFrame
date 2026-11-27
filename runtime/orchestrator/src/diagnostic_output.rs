// SPDX-License-Identifier: MIT
use crate::sync::{lock, try_lock};
use std::cell::Cell;
use std::sync::{Mutex, MutexGuard};

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
        let guard = lock(&OUTPUT);
        IN_OUTPUT.set(true);
        Some(Self { _guard: guard })
    }

    pub(super) fn try_enter() -> Option<Self> {
        if Self::active_here() {
            return None;
        }
        let guard = try_lock(&OUTPUT)?;

        IN_OUTPUT.set(true);
        Some(Self { _guard: guard })
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        IN_OUTPUT.set(false);
    }
}
