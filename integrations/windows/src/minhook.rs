// SPDX-License-Identifier: MIT
//! Bindings to the adapter's MinHook instance, initialized and linked by the adapter.
use std::ffi::c_void;

const OK: i32 = 0;
const ALREADY_DISABLED: i32 = 6;

unsafe extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_QueueEnableHook(target: *mut c_void) -> i32;
    fn MH_QueueDisableHook(target: *mut c_void) -> i32;
    fn MH_ApplyQueued() -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

/// # Safety
/// The target and detour must have compatible machine-level calling conventions and lifetimes.
/// If supplied, original must be writable and the caller must respect the trampoline lifetime.
pub unsafe fn create(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> bool {
    unsafe { MH_CreateHook(target, detour, original) == OK }
}

/// # Safety
/// The target must be an individually owned hook that stays valid until apply.
pub unsafe fn queue(target: *mut c_void, enable: bool) -> bool {
    let status = unsafe {
        if enable {
            MH_QueueEnableHook(target)
        } else {
            MH_QueueDisableHook(target)
        }
    };
    status == OK
}

/// # Safety
/// Every queued hook must be ready to transition.
pub unsafe fn apply() -> bool {
    unsafe { MH_ApplyQueued() == OK }
}

pub(crate) unsafe fn enable(target: *mut c_void) -> bool {
    unsafe { MH_EnableHook(target) == OK }
}

pub(crate) unsafe fn disable(target: *mut c_void) -> bool {
    matches!(unsafe { MH_DisableHook(target) }, OK | ALREADY_DISABLED)
}

pub(crate) unsafe fn remove(target: *mut c_void) -> bool {
    unsafe { MH_RemoveHook(target) == OK }
}
