// SPDX-License-Identifier: MIT
//! MinHook (BSD-2-Clause), the detour library shared by every hooking crate: its declarations and
//! the hde64 disassembler binding. The build script links the pinned source without bundling, so
//! Cargo test binaries get MinHook while each CMake binary links the single `minhook` target.
#![cfg(windows)]
use std::ffi::{c_char, c_void};

#[cfg(target_pointer_width = "64")]
#[allow(non_camel_case_types, non_upper_case_globals, dead_code)]
pub mod hde;

// MinHook.h MH_STATUS values; the functions return them as i32.
pub const MH_UNKNOWN: i32 = -1;
pub const MH_OK: i32 = 0;
pub const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;
pub const MH_ERROR_NOT_INITIALIZED: i32 = 2;
pub const MH_ERROR_ALREADY_CREATED: i32 = 3;
pub const MH_ERROR_NOT_CREATED: i32 = 4;
pub const MH_ERROR_ENABLED: i32 = 5;
pub const MH_ERROR_DISABLED: i32 = 6;
pub const MH_ERROR_NOT_EXECUTABLE: i32 = 7;
pub const MH_ERROR_UNSUPPORTED_FUNCTION: i32 = 8;
pub const MH_ERROR_MEMORY_ALLOC: i32 = 9;
pub const MH_ERROR_MEMORY_PROTECT: i32 = 10;
pub const MH_ERROR_MODULE_NOT_FOUND: i32 = 11;
pub const MH_ERROR_FUNCTION_NOT_FOUND: i32 = 12;

unsafe extern "system" {
    pub fn MH_Initialize() -> i32;
    pub fn MH_Uninitialize() -> i32;
    pub fn MH_CreateHook(
        target: *mut c_void,
        detour: *mut c_void,
        original: *mut *mut c_void,
    ) -> i32;
    pub fn MH_RemoveHook(target: *mut c_void) -> i32;
    pub fn MH_EnableHook(target: *mut c_void) -> i32;
    pub fn MH_DisableHook(target: *mut c_void) -> i32;
    pub fn MH_QueueEnableHook(target: *mut c_void) -> i32;
    pub fn MH_QueueDisableHook(target: *mut c_void) -> i32;
    pub fn MH_ApplyQueued() -> i32;
    pub fn MH_StatusToString(status: i32) -> *const c_char;
}
