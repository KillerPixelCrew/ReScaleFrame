// SPDX-License-Identifier: MIT
//! Observer hook installation, activation, and retryable disable. The session (`session.rs`)
//! owns the control record under its lock; native code keeps the typed MinHook trampolines.
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

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

// Admission and MinHook control run under the session lock. Trampolines are published
// before enable and remain callable until a successful removal of a disabled hook.
static TARGETS: [AtomicUsize; HOOKS.len()] = [const { AtomicUsize::new(0) }; HOOKS.len()];
static ORIGINALS: [AtomicPtr<c_void>; HOOKS.len()] =
    [const { AtomicPtr::new(ptr::null_mut()) }; HOOKS.len()];

unsafe extern "C" {
    fn rsf_skyrim_observer_detour(index: u32) -> *mut c_void;
}

pub(crate) fn bind_targets(targets: [usize; HOOKS.len()]) {
    for (slot, target) in TARGETS.iter().zip(targets) {
        slot.store(target, Ordering::Relaxed);
    }
}

fn target(index: usize) -> *mut c_void {
    ptr::with_exposed_provenance_mut(TARGETS[index].load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_original(index: u32) -> *mut c_void {
    ORIGINALS[index as usize].load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_initialize() -> bool {
    rsf_windows::minhook::initialize()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_create(index: u32) -> bool {
    let detour = unsafe { rsf_skyrim_observer_detour(index) };
    let mut original = ptr::null_mut();
    if !unsafe { rsf_windows::minhook::create(target(index as usize), detour, &mut original) } {
        return false;
    }
    ORIGINALS[index as usize].store(original, Ordering::Release);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_remove(index: u32) -> bool {
    if !unsafe { rsf_windows::minhook::remove_idempotent(target(index as usize)) } {
        return false;
    }
    ORIGINALS[index as usize].store(ptr::null_mut(), Ordering::Release);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_enable(index: u32) -> i32 {
    unsafe { rsf_windows::minhook::enable_status(target(index as usize)) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_disable(index: u32) -> bool {
    unsafe { rsf_windows::minhook::disable(target(index as usize)) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_pin() -> bool {
    let address = unsafe { rsf_skyrim_observer_detour(0) } as usize;
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
