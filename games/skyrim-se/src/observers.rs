// SPDX-License-Identifier: MIT
//! Observer hook installation, activation, and retryable disable. The session (`session.rs`)
//! owns the control record under its lock; native code keeps the typed MinHook trampolines.
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CString, c_char, c_void};

use crate::hook_set::HookSet;

const HOOKS: [&str; 5] = [
    "jitter",
    "process-main",
    "resolution",
    "renderer-end",
    "frame-upload",
];
#[derive(Default)]
pub(crate) struct Control {
    hooks: HookSet<{ HOOKS.len() }>,
    pinned: bool,
}

/// `rsf_skyrim_observer_leaves`, lent by `plugin.cpp` for the process.
#[repr(C)]
pub(crate) struct Leaves {
    pub(crate) initialize: unsafe extern "C" fn() -> bool,
    pub(crate) create: unsafe extern "C" fn(u32) -> bool,
    pub(crate) remove: unsafe extern "C" fn(u32) -> bool,
    pub(crate) enable: unsafe extern "C" fn(u32) -> i32,
    pub(crate) disable: unsafe extern "C" fn(u32) -> bool,
    pub(crate) pin: unsafe extern "C" fn() -> bool,
    pub(crate) log: unsafe extern "C" fn(*const rsf_game_host_services, *const c_char),
}

const _: () = assert!(size_of::<Leaves>() == 56);

impl Control {
    pub(crate) const fn new() -> Self {
        Self {
            hooks: HookSet::new(),
            pinned: false,
        }
    }

    /// Roll back only disabled hooks, retaining any whose removal refused.
    fn remove_created(&mut self, leaves: &Leaves) {
        self.hooks.remove((0..HOOKS.len()).rev(), |index| unsafe {
            (leaves.remove)(index as u32)
        });
    }

    pub(crate) fn prepare(&mut self, leaves: &Leaves) -> bool {
        if self.pinned {
            return self.hooks.first_unavailable().is_none();
        }
        self.remove_created(leaves);
        if !self.hooks.is_empty() || !unsafe { (leaves.initialize)() } {
            return false;
        }
        if !self
            .hooks
            .create(|index| unsafe { (leaves.create)(index as u32) })
        {
            self.remove_created(leaves);
            return false;
        }
        if !unsafe { (leaves.pin)() } {
            self.remove_created(leaves);
            return false;
        }
        self.pinned = true;
        true
    }

    pub(crate) fn disable(&mut self, leaves: &Leaves, rollback: bool) -> bool {
        let count = HOOKS.len();
        let order = (0..count).map(|ordinal| {
            if rollback {
                count - 1 - ordinal
            } else {
                ordinal
            }
        });
        self.hooks
            .disable(order, |index| unsafe { (leaves.disable)(index as u32) })
    }

    pub(crate) fn start(&mut self, leaves: &Leaves, host: &rsf_game_host_services) -> bool {
        if !self.pinned || !self.hooks.is_complete() || !self.disable(leaves, true) {
            return false;
        }
        if let Err((index, status)) = self
            .hooks
            .enable(|index| unsafe { (leaves.enable)(index as u32) })
        {
            let stage = HOOKS[index];
            if let Ok(message) = CString::new(format!(
                "Skyrim start refused at {stage}: MinHook status={status}"
            )) {
                unsafe { (leaves.log)(host, message.as_ptr()) };
            }
            self.disable(leaves, true);
            return false;
        }
        true
    }
}

// These leaves bind the existing observer transaction to the same Rust MinHook owner as
// cold creation and CPU hooks. The adapter supplies typed native detours and trampoline slots.
#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_initialize() -> bool {
    rsf_windows::minhook::initialize()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_observer_create(
    target: *mut c_void,
    detour: *mut c_void,
    original: *mut *mut c_void,
) -> bool {
    unsafe { rsf_windows::minhook::create(target, detour, original) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_observer_remove(
    target: *mut c_void,
    original: &mut *mut c_void,
) -> bool {
    if !unsafe { rsf_windows::minhook::remove_idempotent(target) } {
        return false;
    }
    *original = std::ptr::null_mut();
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_observer_enable(target: *mut c_void) -> i32 {
    unsafe { rsf_windows::minhook::enable_status(target) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_observer_disable(target: *mut c_void) -> bool {
    unsafe { rsf_windows::minhook::disable(target) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_pin(address: usize) -> bool {
    crate::platform::get().is_some_and(|platform| platform.pin_module(address) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static DISABLE_REFUSES: Cell<bool> = const { Cell::new(true) };
    }

    extern "C" fn yes() -> bool {
        true
    }

    extern "C" fn hook_yes(_: u32) -> bool {
        true
    }

    extern "C" fn enable(index: u32) -> i32 {
        i32::from(index == 1)
    }

    extern "C" fn disable(index: u32) -> bool {
        index != 0 || !DISABLE_REFUSES.get()
    }

    extern "C" fn log(_: *const rsf_game_host_services, _: *const c_char) {}

    #[test]
    fn a_failed_start_keeps_hooks_whose_disable_refuses() {
        let leaves = Leaves {
            initialize: yes,
            create: hook_yes,
            remove: hook_yes,
            enable,
            disable,
            pin: yes,
            log,
        };
        let mut control = Control::default();
        // The fake log does not read the host. Zero is an empty GameSDK services table.
        let host = unsafe { std::mem::zeroed() };
        assert!(control.prepare(&leaves));
        assert!(!control.start(&leaves, &host));
        assert_eq!(control.hooks.first_unavailable(), Some(0));
        assert!(!control.disable(&leaves, false));
        DISABLE_REFUSES.set(false);
        assert!(control.disable(&leaves, false));
        assert!(control.hooks.is_disabled());
    }
}
