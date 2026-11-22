// SPDX-License-Identifier: MIT
//! Windows loader bindings and the C entry points called by bridge.cpp.

use crate::frame_generation::{Options, Platform as FgPlatform};
use crate::lifecycle::{Lifecycle, Platform, System, feature_mask};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{NonNull, null_mut};

const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x100;
const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x1000;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

struct Win32;

impl System for Win32 {
    unsafe fn load(&self, path: *const u16) -> *mut c_void {
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        unsafe { LoadLibraryExW(path, null_mut(), flags) }
    }

    unsafe fn free(&self, module: NonNull<c_void>) {
        unsafe { FreeLibrary(module.as_ptr()) };
    }

    unsafe fn close(&self, handle: *mut c_void) {
        unsafe { CloseHandle(handle) };
    }

    unsafe fn symbol(&self, module: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { GetProcAddress(module, name) }
    }

    unsafe fn loaded(&self, path: *const u16) -> *mut c_void {
        unsafe { GetModuleHandleW(path) }
    }
}

static LIFECYCLE: Lifecycle = Lifecycle::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_diagnostic_present_error(result: i32) {
    LIFECYCLE.diagnostics.present_error(result);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_diagnostic_take_present_error() -> i32 {
    LIFECYCLE.diagnostics.take_present_error()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_diagnostic_worker_timeout_epoch() -> u64 {
    LIFECYCLE.diagnostics.worker_timeout_epoch()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_diagnostic_set_verbose(verbose: u32) {
    LIFECYCLE.diagnostics.set_verbose(verbose != 0);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_diagnostic_message(message: *const c_char, info: u32) -> u32 {
    if message.is_null() {
        return 0;
    }
    let message = unsafe { CStr::from_ptr(message) };
    u32::from(LIFECYCLE.diagnostics.message(message.to_bytes(), info != 0))
}

/// Loads the verified interposer if needed and initializes the selected features.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_initialize(
    platform: &Platform,
    directory: *const u16,
    project: *const c_char,
    reflex: u32,
    frame_generation: u32,
    super_resolution: u32,
) -> i32 {
    let features = feature_mask(reflex != 0, frame_generation != 0, super_resolution != 0);
    unsafe { LIFECYCLE.initialize(&Win32, platform, directory, project, features) }
}

/// Shuts the SDK down once. The result is sticky; the module stays loaded.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_shutdown(platform: &Platform) -> i32 {
    unsafe { LIFECYCLE.shutdown(platform) }
}

/// Unloads the interposer unless shutdown failed.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_unload(platform: &Platform) -> i32 {
    unsafe { LIFECYCLE.unload(&Win32, platform) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_initialized() -> u32 {
    u32::from(LIFECYCLE.initialized())
}

/// The interposer while the SDK is initialized, otherwise null.
#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_module() -> *mut c_void {
    LIFECYCLE.module()
}

/// vkGetInstanceProcAddr of the loaded interposer. Kept after slShutdown.
#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_instance_proc() -> *mut c_void {
    LIFECYCLE.instance_proc()
}

/// vkGetDeviceProcAddr of the loaded interposer. Kept after slShutdown.
#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_device_proc() -> *mut c_void {
    LIFECYCLE.device_proc()
}

/// Resolves an export of the loaded interposer without another module reference.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_interposer_symbol(name: *const c_char) -> *mut c_void {
    unsafe { LIFECYCLE.interposer_symbol(&Win32, name) }
}

/// Resolves an export of sl.common.dll from the plugin directory if it is loaded.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_common_symbol(name: *const c_char) -> *mut c_void {
    unsafe { LIFECYCLE.common_symbol(&Win32, name) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_fg_swapchain_created(swapchain: u64) {
    LIFECYCLE.swapchain().created(swapchain);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_fg_swapchain_destroyed(swapchain: u64) {
    LIFECYCLE.swapchain().destroyed(swapchain);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_fg_set(
    platform: &FgPlatform,
    enabled: i32,
    options: &Options,
) -> i32 {
    let enabled = enabled != 0;
    let inactive = if enabled { -1 } else { 0 };
    LIFECYCLE.with_frame_generation(inactive, |fg, revision| unsafe {
        fg.set(platform, revision, enabled, *options)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_fg_disable(platform: &FgPlatform) -> i32 {
    LIFECYCLE.with_frame_generation(0, |fg, revision| unsafe { fg.disable(platform, revision) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_fg_free(platform: &FgPlatform) -> i32 {
    LIFECYCLE.with_frame_generation(0, |fg, _| unsafe { fg.free(platform) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_sdk_call(
    call: unsafe extern "C" fn(*mut c_void) -> i32,
    user: *mut c_void,
    inactive: i32,
) -> i32 {
    LIFECYCLE.with_sdk(inactive, || unsafe { call(user) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_reflex_options(
    apply: unsafe extern "C" fn(u32, u32) -> i32,
    mode: i32,
    limit: u32,
    limit_is_fps: u32,
) -> i32 {
    let mode = match mode {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    let interval = if limit_is_fps == 0 {
        limit
    } else {
        1_000_000u32.checked_div(limit).unwrap_or(0)
    };
    LIFECYCLE.with_sdk(-1, || unsafe { apply(mode, interval) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_token_begin(
    acquire: unsafe extern "C" fn(u32, *mut usize) -> i32,
    index: u32,
    output: &mut usize,
) -> i32 {
    *output = 0;
    match LIFECYCLE
        .frame_token
        .begin(|token| unsafe { acquire(index, token) })
    {
        Ok(token) => {
            *output = token;
            0
        }
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_token_current() -> usize {
    LIFECYCLE.frame_token.current()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_token_call(
    explicit: usize,
    call: unsafe extern "C" fn(usize, *mut c_void) -> i32,
    user: *mut c_void,
    inactive: i32,
) -> i32 {
    LIFECYCLE
        .frame_token
        .with_token(explicit, inactive, |token| unsafe { call(token, user) })
}
