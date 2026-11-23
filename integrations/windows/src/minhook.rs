// SPDX-License-Identifier: MIT
//! The adapter's MinHook instance, initialized and linked by the adapter.
use std::ffi::c_void;

const OK: i32 = 0;
const ALREADY_INITIALIZED: i32 = 1;
const NOT_CREATED: i32 = 4;
const ALREADY_ENABLED: i32 = 5;
const ALREADY_DISABLED: i32 = 6;

use rsf_minhook_sys::{
    MH_ApplyQueued, MH_CreateHook, MH_DisableHook, MH_EnableHook, MH_Initialize,
    MH_QueueDisableHook, MH_QueueEnableHook, MH_RemoveHook,
};

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

// The XInput facts' enable and remove; the public calls below report the other outcomes.
#[cfg(feature = "xinput-facts")]
pub(crate) unsafe fn enable(target: *mut c_void) -> bool {
    unsafe { MH_EnableHook(target) == OK }
}

/// # Safety
/// The target must identify a hook owned by the caller.
pub unsafe fn disable(target: *mut c_void) -> bool {
    matches!(unsafe { MH_DisableHook(target) }, OK | ALREADY_DISABLED)
}

#[cfg(feature = "xinput-facts")]
pub(crate) unsafe fn remove(target: *mut c_void) -> bool {
    unsafe { MH_RemoveHook(target) == OK }
}

/// Initialize or join the linked MinHook instance.
pub fn initialize() -> bool {
    matches!(unsafe { MH_Initialize() }, OK | ALREADY_INITIALIZED)
}

/// # Safety
/// The caller owns the target and has published its original trampoline.
pub unsafe fn enable_status(target: *mut c_void) -> i32 {
    unsafe { MH_EnableHook(target) }
}

/// # Safety
/// The caller owns the target and has published its original trampoline.
pub unsafe fn enable_idempotent(target: *mut c_void) -> bool {
    matches!(unsafe { enable_status(target) }, OK | ALREADY_ENABLED)
}

/// # Safety
/// The caller owns the disabled target and will no longer use its trampoline.
pub unsafe fn remove_idempotent(target: *mut c_void) -> bool {
    matches!(unsafe { MH_RemoveHook(target) }, OK | NOT_CREATED)
}
