// SPDX-License-Identifier: MIT
use super::*;
use windows::{
    Win32::{
        Foundation::HMODULE,
        System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW,
        },
    },
    core::PCWSTR,
};

const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;

use rsf_minhook_sys::{MH_CreateHook, MH_EnableHook, MH_Initialize, MH_RemoveHook};

/// Creates the disabled hook and returns its trampoline.
pub(super) unsafe fn create_hook(
    target: *mut c_void,
    detour: CapabilityFn,
) -> Option<CapabilityFn> {
    if !matches!(
        unsafe { MH_Initialize() },
        MH_OK | MH_ERROR_ALREADY_INITIALIZED
    ) {
        return None;
    }
    let mut original = ptr::null_mut();
    if unsafe { MH_CreateHook(target, detour as *mut c_void, &mut original) } != MH_OK
        || original.is_null()
    {
        return None;
    }
    // MinHook returns a trampoline with the target capability signature.
    Some(unsafe { std::mem::transmute::<*mut c_void, CapabilityFn>(original) })
}

/// Pins both code owners and enables the hook, returning the SDK module. On
/// failure, removes the disabled hook and returns null.
pub(super) unsafe fn enable_hook(target: *mut c_void, detour: CapabilityFn) -> *mut c_void {
    let pin = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
    let mut implementation = HMODULE::default();
    let mut runtime = HMODULE::default();
    // The detour and SDK must outlive every call through the resident trampoline.
    if unsafe {
        GetModuleHandleExW(pin, PCWSTR(detour as *const u16), &mut implementation).is_ok()
            && GetModuleHandleExW(pin, PCWSTR(target.cast()), &mut runtime).is_ok()
            && MH_EnableHook(target) == MH_OK
    } {
        return runtime.0;
    }
    unsafe { MH_RemoveHook(target) };
    ptr::null_mut()
}
