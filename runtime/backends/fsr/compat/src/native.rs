// SPDX-License-Identifier: MIT
use super::{dx12::*, fingerprint::*, *};
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

unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

unsafe extern "C" fn create_hook(
    target: *mut c_void,
    detour: rsf_fsr4_compat_capability_fn,
) -> rsf_fsr4_compat_capability_fn {
    let detour = detour?;
    if !matches!(
        unsafe { MH_Initialize() },
        MH_OK | MH_ERROR_ALREADY_INITIALIZED
    ) {
        return None;
    }
    let mut original = ptr::null_mut();
    if unsafe { MH_CreateHook(target, detour as *mut c_void, &mut original) } != MH_OK {
        return None;
    }
    // MinHook returns a trampoline with the target capability signature.
    unsafe { std::mem::transmute::<*mut c_void, rsf_fsr4_compat_capability_fn>(original) }
}

unsafe extern "C" fn enable_hook(
    target: *mut c_void,
    detour: rsf_fsr4_compat_capability_fn,
) -> *mut c_void {
    let Some(detour) = detour else {
        return ptr::null_mut();
    };
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

static PLATFORM: rsf_fsr4_compat_platform = rsf_fsr4_compat_platform {
    struct_size: size_of::<rsf_fsr4_compat_platform>() as u32,
    abi_version: RSF_FSR4_COMPAT_PLATFORM_ABI_VERSION,
    adapter_vendor: Some(rsf_fsr4_compat_adapter_vendor),
    device_supports_int8: Some(rsf_fsr4_compat_device_supports_int8),
    retain_device: Some(rsf_fsr4_compat_retain_device),
    release_device: Some(rsf_fsr4_compat_release_device),
    module_digest: Some(rsf_fsr4_compat_module_digest),
    match_entry: Some(rsf_fsr4_compat_match_entry),
    create_hook: Some(create_hook),
    enable_hook: Some(enable_hook),
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_acquire_native_adapter(
    module: *mut c_void,
    adapter: *mut c_void,
    device: *mut c_void,
    log: rsf_fsr4_compat_log_fn,
    user: *mut c_void,
) -> *const rsf_fsr4_compat_lease {
    unsafe { rsf_fsr4_compat_acquire_logged(&PLATFORM, module, adapter, device, log, user) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_acquire_native_device(
    module: *mut c_void,
    device: *mut c_void,
    log: rsf_fsr4_compat_log_fn,
    user: *mut c_void,
) -> *const rsf_fsr4_compat_lease {
    unsafe { acquire_device(&PLATFORM, module, device, log, user) }
}
