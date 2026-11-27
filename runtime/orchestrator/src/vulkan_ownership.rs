// SPDX-License-Identifier: MIT
use std::sync::atomic::{AtomicBool, Ordering};

/// Holds a process-local Vulkan owner flag for the lifetime of its owner.
pub(super) struct Claim(&'static AtomicBool);

impl Claim {
    pub(super) fn acquire(flag: &'static AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
