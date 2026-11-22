// SPDX-License-Identifier: MIT
//! Typed Streamline 2.14.1 entry points for the Rust call leaves.
//!
//! The interposer exports its API with C linkage, and the references in the
//! C++ declarations are pointers in that ABI. The records are the C mirrors in
//! sdk_records.h, sdk_fg_options.h and sdk_sr_options.h, which
//! sdk_records_checks.h checks against the pinned C++ headers. The entry
//! types use the C-unwind ABI and each call runs inside `guarded`, whose C
//! leaf turns a foreign C++ exception into eErrorExceptionHandler.
//!
//! The Runtime orchestrator includes this file with `#[path]`; `super` holds
//! `sdk_records_abi` in both crates.
#![allow(dead_code)] // Each crate calls the entries it owns.

use crate::sys::{rsf_slvk_sdk_adapter, rsf_slvk_sdk_header, rsf_slvk_sdk_viewport};
use std::ffi::{CStr, c_char, c_void};
use std::mem::{size_of, transmute_copy};
use std::ptr;

pub(crate) const OK: i32 = 0;
/// `sl::Result::eErrorExceptionHandler`.
pub(crate) const EXCEPTION_HANDLER: i32 = 24;
/// `sl::Result::eWarnOutOfVRAM`.
pub(crate) const OUT_OF_VRAM: i32 = 39;
pub(crate) const FEATURE_DLSS: u32 = 0;
pub(crate) const FEATURE_REFLEX: u32 = 3;
pub(crate) const FEATURE_PCL: u32 = 4;
pub(crate) const FEATURE_DLSS_G: u32 = 1000;
/// `sl::ResourceLifecycle::eValidUntilEvaluate`.
pub(crate) const VALID_UNTIL_EVALUATE: u32 = 2;

/// A borrowed `sl::FrameToken`.
pub(crate) type Token = *const c_void;
pub(crate) type Commands = *mut c_void;

pub(crate) type IsFeatureSupported =
    unsafe extern "C-unwind" fn(u32, *const rsf_slvk_sdk_adapter) -> i32;
pub(crate) type GetFeatureFunction =
    unsafe extern "C-unwind" fn(u32, *const c_char, *mut *mut c_void) -> i32;
pub(crate) type SetConstants<C> =
    unsafe extern "C-unwind" fn(*const C, Token, *const rsf_slvk_sdk_viewport) -> i32;
pub(crate) type SetTagForFrame<T> = unsafe extern "C-unwind" fn(
    Token,
    *const rsf_slvk_sdk_viewport,
    *const T,
    u32,
    Commands,
) -> i32;
pub(crate) type EvaluateFeature = unsafe extern "C-unwind" fn(
    u32,
    Token,
    *const *const rsf_slvk_sdk_header,
    u32,
    Commands,
) -> i32;
pub(crate) type FreeResources =
    unsafe extern "C-unwind" fn(u32, *const rsf_slvk_sdk_viewport) -> i32;
/// `slDLSSSetOptions` and `slDLSSGSetOptions`.
pub(crate) type SetOptions<O> =
    unsafe extern "C-unwind" fn(*const rsf_slvk_sdk_viewport, *const O) -> i32;
pub(crate) type GetOptimalSettings<O, S> = unsafe extern "C-unwind" fn(*const O, *mut S) -> i32;
pub(crate) type GetFgState<S, O> =
    unsafe extern "C-unwind" fn(*const rsf_slvk_sdk_viewport, *mut S, *const O) -> i32;
pub(crate) type Init<P> = unsafe extern "C-unwind" fn(*const P, u64) -> i32;
pub(crate) type Shutdown = unsafe extern "C-unwind" fn() -> i32;
pub(crate) type GetNewFrameToken = unsafe extern "C-unwind" fn(*mut Token, *const u32) -> i32;
/// `slReflexSetOptions`, `slPCLSetOptions`, `slReflexGetState` and `slPCLGetState`.
pub(crate) type Apply<R> = unsafe extern "C-unwind" fn(*const R) -> i32;
pub(crate) type Read<R> = unsafe extern "C-unwind" fn(*mut R) -> i32;
pub(crate) type ReflexSleep = unsafe extern "C-unwind" fn(Token) -> i32;
pub(crate) type PclSetMarker = unsafe extern "C-unwind" fn(u32, Token) -> i32;
/// `PFunOnAPIErrorCallback`, which receives `const APIError&`.
pub(crate) type ApiErrorCallback = unsafe extern "C" fn(*const i32);

pub(crate) const fn header(
    type_lo: u32,
    type_mid: u16,
    type_hi: u16,
    type_tail: [u8; 8],
    version: usize,
) -> rsf_slvk_sdk_header {
    rsf_slvk_sdk_header {
        next: ptr::null_mut(),
        type_lo,
        type_mid,
        type_hi,
        type_tail,
        version,
    }
}

/// `sl::Resource` and `sl::ResourceTag` headers.
pub(crate) const RESOURCE: rsf_slvk_sdk_header = header(
    0x3a9d_70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
    1,
);
pub(crate) const TAG: rsf_slvk_sdk_header = header(
    0x4c6a_5aad,
    0xb445,
    0x496c,
    [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
    1,
);

pub(crate) const fn viewport(value: u32) -> rsf_slvk_sdk_viewport {
    rsf_slvk_sdk_viewport {
        base: header(
            0x171b_6435,
            0x9b3c,
            0x4fc8,
            [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
            1,
        ),
        value,
    }
}

/// Selects the adapter by its Vulkan physical device only.
pub(crate) const fn adapter(physical_device: *mut c_void) -> rsf_slvk_sdk_adapter {
    rsf_slvk_sdk_adapter {
        base: header(
            0x0677_315f,
            0xa746,
            0x4492,
            [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
            1,
        ),
        luid: ptr::null_mut(),
        luid_size: 0,
        physical_device,
    }
}

/// Reads an entry point. Null resolves to None.
///
/// # Safety
/// `T` must be the function pointer type of the named entry.
pub(crate) unsafe fn function<T: Copy>(address: *mut c_void) -> Option<T> {
    const { assert!(size_of::<T>() == size_of::<*mut c_void>()) };
    (!address.is_null()).then(|| unsafe { transmute_copy(&address) })
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// Resolves an export from the module without taking a module reference.
///
/// # Safety
/// `module` is a loaded module and `T` the function pointer type of `name`.
#[cfg(windows)]
pub(crate) unsafe fn export<T: Copy>(module: *mut c_void, name: &CStr) -> Option<T> {
    if module.is_null() {
        return None;
    }
    unsafe { function(GetProcAddress(module, name.as_ptr())) }
}

/// Resolves one feature function only when Streamline reports success and a
/// nonnull address.
///
/// # Safety
/// `T` must be the function pointer type of the named feature function.
pub(crate) unsafe fn feature<T: Copy>(get: GetFeatureFunction, id: u32, name: &CStr) -> Option<T> {
    let mut address = ptr::null_mut();
    let result = guarded(|| unsafe { get(id, name.as_ptr(), &mut address) });
    if result != OK {
        return None;
    }
    unsafe { function(address) }
}

type GuardedCall = unsafe extern "C-unwind" fn(*mut c_void) -> i32;

unsafe extern "C" {
    // sdk_guard.c. Each crate's build script compiles its own copy, and the
    // Runtime orchestrator's copy has a distinct name.
    #[cfg_attr(rsf_streamline_core, link_name = "rsf_core_slvk_guarded")]
    fn rsf_slvk_guarded(call: GuardedCall, user: *mut c_void, thrown: i32) -> i32;
}

/// Runs one SDK call. A foreign C++ exception from the SDK returns
/// eErrorExceptionHandler.
pub(crate) fn guarded<F: FnMut() -> i32>(mut call: F) -> i32 {
    unsafe extern "C-unwind" fn thunk<F: FnMut() -> i32>(user: *mut c_void) -> i32 {
        unsafe { (*user.cast::<F>())() }
    }
    unsafe { rsf_slvk_guarded(thunk::<F>, (&raw mut call).cast(), EXCEPTION_HANDLER) }
}
