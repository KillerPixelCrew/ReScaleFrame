// SPDX-License-Identifier: MIT
//! Windows loader bindings, the bridge exports and the lifecycle entry points
//! the native Vulkan services read.

mod bridge;
mod wsi;

pub(crate) use bridge::{fg_apply, fg_options_bound, fg_release};

use crate::lifecycle::{Lifecycle, System};
use std::ffi::{CStr, c_char, c_void};
use std::io::{self, Write};
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
    fn GetCurrentThreadId() -> u32;
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

    fn verbose(&self) -> bool {
        std::env::var_os("VULKANSTORY_STREAMLINE_VERBOSE").is_some_and(|value| !value.is_empty())
    }
}

static LIFECYCLE: Lifecycle = Lifecycle::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_take_present_error() -> i32 {
    LIFECYCLE.diagnostics.take_present_error()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_worker_timeout_epoch() -> u64 {
    LIFECYCLE.diagnostics.worker_timeout_epoch()
}

/// Streamline's log callback (`sl::LogType`, message).
unsafe extern "C" fn rsf_slvk_log_message(kind: u32, message: *const c_char) {
    if message.is_null() {
        return;
    }
    let message = unsafe { CStr::from_ptr(message) };
    if !LIFECYCLE.diagnostics.message(message.to_bytes(), kind == 0) {
        return;
    }
    let level = match kind {
        2 => "error",
        1 => "warning",
        _ => "info",
    };
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "[Streamline {level}] ");
    let _ = stderr.write_all(message.to_bytes());
    let _ = stderr.write_all(b"\n");
}

/// The interposer while the SDK is initialized, otherwise null.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_module() -> *mut c_void {
    LIFECYCLE.module()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_current_frame_token() -> usize {
    LIFECYCLE.frame_token.current()
}
