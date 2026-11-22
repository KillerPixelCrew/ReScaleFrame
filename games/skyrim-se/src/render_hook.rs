// SPDX-License-Identifier: MIT
//! Publication of a permanent render trampoline and its replaceable session binding.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, PoisonError};

pub(crate) struct RenderHook<T> {
    pub(crate) original: AtomicPtr<c_void>,
    pub(crate) live: AtomicBool,
    binding: Mutex<Option<T>>,
}

impl<T: Copy> RenderHook<T> {
    pub(crate) const fn new() -> Self {
        Self {
            original: AtomicPtr::new(ptr::null_mut()),
            live: AtomicBool::new(false),
            binding: Mutex::new(None),
        }
    }

    pub(crate) fn binding(&self) -> Option<T> {
        *self.binding.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // The session serializes writers. The detour can observe its binding while enable runs.
    pub(crate) fn enable(
        &self,
        binding: T,
        create: impl FnOnce() -> Option<*mut c_void>,
        enable: impl FnOnce() -> bool,
    ) -> bool {
        if self.original.load(Ordering::Acquire).is_null() {
            let Some(original) = create() else {
                return false;
            };
            self.original.store(original, Ordering::Release);
        }
        let previous = self
            .binding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(binding);
        if !enable() {
            *self.binding.lock().unwrap_or_else(PoisonError::into_inner) = previous;
            return false;
        }
        self.live.store(true, Ordering::Release);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_enable_restores_binding_and_keeps_trampoline() {
        let hook = RenderHook::new();
        let original = ptr::NonNull::<c_void>::dangling().as_ptr();
        assert!(!hook.enable(
            1,
            || Some(original),
            || {
                assert_eq!(hook.binding(), Some(1));
                assert_eq!(hook.original.load(Ordering::Acquire), original);
                false
            }
        ));
        assert_eq!(hook.binding(), None);
        assert!(!hook.live.load(Ordering::Acquire));

        assert!(hook.enable(2, || panic!("trampoline already exists"), || true));
        assert!(!hook.enable(3, || panic!("trampoline already exists"), || false));
        assert_eq!(hook.binding(), Some(2));
        assert!(hook.live.load(Ordering::Acquire));
    }
}
