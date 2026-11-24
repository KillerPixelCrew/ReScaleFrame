// SPDX-License-Identifier: MIT
//! The adapter's MinHook instance, initialized and linked by the adapter.
use std::ffi::c_void;

use rsf_minhook_sys::{
    MH_ApplyQueued, MH_CreateHook, MH_DisableHook, MH_ERROR_ALREADY_INITIALIZED, MH_ERROR_DISABLED,
    MH_ERROR_ENABLED, MH_ERROR_NOT_CREATED, MH_EnableHook, MH_Initialize, MH_OK,
    MH_QueueDisableHook, MH_QueueEnableHook, MH_RemoveHook,
};

/// # Safety
/// The target and detour must have compatible machine-level calling conventions and lifetimes.
/// If supplied, original must be writable and the caller must respect the trampoline lifetime.
pub unsafe fn create(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> bool {
    unsafe { MH_CreateHook(target, detour, original) == MH_OK }
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
    status == MH_OK
}

/// # Safety
/// Every queued hook must be ready to transition.
pub unsafe fn apply() -> bool {
    unsafe { MH_ApplyQueued() == MH_OK }
}

// The XInput facts' enable and remove; the public calls below report the other outcomes.
#[cfg(feature = "xinput-facts")]
pub(crate) unsafe fn enable(target: *mut c_void) -> bool {
    unsafe { MH_EnableHook(target) == MH_OK }
}

/// # Safety
/// The target must identify a hook owned by the caller.
pub unsafe fn disable(target: *mut c_void) -> bool {
    matches!(unsafe { MH_DisableHook(target) }, MH_OK | MH_ERROR_DISABLED)
}

#[cfg(feature = "xinput-facts")]
pub(crate) unsafe fn remove(target: *mut c_void) -> bool {
    unsafe { MH_RemoveHook(target) == MH_OK }
}

/// Initialize or join the linked MinHook instance.
pub fn initialize() -> bool {
    matches!(
        unsafe { MH_Initialize() },
        MH_OK | MH_ERROR_ALREADY_INITIALIZED
    )
}

/// # Safety
/// The caller owns the target and has published its original trampoline.
pub unsafe fn enable_status(target: *mut c_void) -> i32 {
    unsafe { MH_EnableHook(target) }
}

/// # Safety
/// The caller owns the target and has published its original trampoline.
pub unsafe fn enable_idempotent(target: *mut c_void) -> bool {
    matches!(unsafe { enable_status(target) }, MH_OK | MH_ERROR_ENABLED)
}

/// # Safety
/// The caller owns the disabled target and will no longer use its trampoline.
pub unsafe fn remove_idempotent(target: *mut c_void) -> bool {
    matches!(
        unsafe { MH_RemoveHook(target) },
        MH_OK | MH_ERROR_NOT_CREATED
    )
}
