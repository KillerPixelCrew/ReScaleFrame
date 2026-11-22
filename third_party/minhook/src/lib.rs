// SPDX-License-Identifier: MIT
//! MinHook (BSD-2-Clause), the detour library shared by every hooking crate: its declarations and
//! the hde64 disassembler binding. The build script links the pinned source without bundling, so
//! Cargo test binaries get MinHook while each CMake binary links the single `minhook` target.
#![cfg(windows)]
use std::ffi::{c_char, c_void};

#[cfg(target_pointer_width = "64")]
#[allow(non_camel_case_types, non_upper_case_globals, dead_code)]
pub mod hde;

// MinHook.h; MH_STATUS values are returned as i32.
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
