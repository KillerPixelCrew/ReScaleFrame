// SPDX-License-Identifier: MIT
//! Fallout 4 AE 1.11.240 adapter policy: the exact recognized build, the GameSDK table, the plugin
//! lifecycle with its process-lived Runtime session, DPI startup, the cold native D3D11
//! creation and presentation binding sequence, the CPU event owner (poll and worker source
//! handoff, input receipts and their GameSDK facts), native depth-domain admission and
//! composition, native input/output resolution scopes, the render hook lifecycle with World/Present
//! admission, the reduced-render recovery transaction and SR and HUDless publication, and
//! render-input admission with its camera history and leased passes.
//!
//! Native leaves stay in C++ and reach this crate through linked host and CPU functions
//! (fallout4_platform.h and cpu_events_platform.h), the render hook
//! leaves (render_hooks_platform.h), and the linked depth-domain and render-input diagnostics:
//! the typed detour shells with
//! their return addresses and trampoline calls, exception guards, the shared file hash, the Steam
//! chain facade, and the render producer callbacks and recovery leaves. Rust samples the
//! typed D3D11 and COM facts, and transfers admitted references to the native World.
//! The Runtime decides vendors, presentation and pacing; this crate reports native facts only.
#![cfg(windows)]

mod build_layout;
mod camera;
mod cpu_abi;
mod cpu_events;
mod creation;
mod d3d11;
mod depth_domains;
mod dpi;
mod platform_abi;
mod render_hooks;
mod render_inputs;
mod render_records;
mod resolution;
mod resolution_stages;
mod screen;
mod session;

use platform_abi::{
    rsf_d3d11_creation_scope, rsf_d3d11_host_setup, rsf_d3d11_native_chain,
    rsf_d3d11_presentation_binding, rsf_fallout4_bind_presentation_fn,
    rsf_fallout4_creation_begin_fn, rsf_fallout4_creation_end_fn, rsf_fallout4_host_active_fn,
    rsf_fallout4_host_create_fn,
};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_hooks_api, rsf_game_host_services, rsf_game_info,
    rsf_game_log_fn, rsf_game_plugin_api, rsf_game_probe, rsf_result,
};
use std::ffi::{CStr, CString, c_void};
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use std::sync::OnceLock;
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetModuleHandleW,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const RSF_OK: rsf_result = 0;
const RSF_ERROR_INVALID_ARGUMENT: rsf_result = -1;
const RSF_ERROR_ABI_MISMATCH: rsf_result = -2;
const RSF_ERROR_NOT_READY: rsf_result = -3;
const RSF_ERROR_BUSY: rsf_result = -4;
const RSF_ERROR_NATIVE_REFUSED: rsf_result = -5;
const UNKNOWN: rsf_detection = 0;
const RECOGNIZED: rsf_detection = 1;
const AMD64: u16 = 0x8664;

// The only recognized build: Fallout 4 AE 1.11.240.0. Every RVA and byte in this crate belongs
// to it.
const EXECUTABLE: &CStr = c"Fallout4.exe";
const EXECUTABLE_SHA256: &CStr =
    c"fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const PENDING: &CStr = c"Guarded native/reduced SR callbacks; game quality, FG admission and measured latency pending validation.";

/// A native leaf threw; the lifecycle entry reports RSF_ERROR_NOT_READY as the C++ catch did.
pub(crate) struct Threw;

/// Operations provided by the linked native guards and MinHook.
pub(crate) struct Ops;

// MinHook.h, MH_STATUS. The native CMake target links the same MinHook instance for all seams.
const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;
const MH_ERROR_DISABLED: i32 = 6;

unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
}

unsafe extern "C" {
    fn rsf_fallout4_native_log(
        log: rsf_game_log_fn,
        user: *mut ::core::ffi::c_void,
        message: *const ::core::ffi::c_char,
    ) -> bool;
    fn rsf_fallout4_native_host_create(
        create: rsf_fallout4_host_create_fn,
        setup: *const rsf_d3d11_host_setup,
        services: *mut rsf_game_host_services,
        result: *mut i32,
    ) -> bool;
    fn rsf_fallout4_native_host_set_active(
        activate: rsf_fallout4_host_active_fn,
        services_user: *mut ::core::ffi::c_void,
        active: u32,
        result: *mut i32,
    ) -> bool;
    fn rsf_fallout4_native_creation_begin(
        begin: rsf_fallout4_creation_begin_fn,
        services_user: *mut ::core::ffi::c_void,
        scope: *mut rsf_d3d11_creation_scope,
        result: *mut i32,
    ) -> bool;
    fn rsf_fallout4_native_creation_end(
        end: rsf_fallout4_creation_end_fn,
        services_user: *mut ::core::ffi::c_void,
        scope: *const rsf_d3d11_creation_scope,
        outputs: *const rsf_d3d11_native_chain,
        result: *mut i32,
    ) -> bool;
    fn rsf_fallout4_native_bind_presentation(
        bind: rsf_fallout4_bind_presentation_fn,
        services_user: *mut ::core::ffi::c_void,
        binding: *const rsf_d3d11_presentation_binding,
        result: *mut i32,
    ) -> bool;
}

static OPS: OnceLock<Ops> = OnceLock::new();

pub(crate) fn ops() -> Option<&'static Ops> {
    OPS.get()
}

fn completed(returned: bool) -> Result<(), Threw> {
    if returned { Ok(()) } else { Err(Threw) }
}

impl Ops {
    pub(crate) fn log(
        &self,
        log: rsf_game_log_fn,
        user: *mut c_void,
        message: &CStr,
    ) -> Result<(), Threw> {
        completed(unsafe { rsf_fallout4_native_log(log, user, message.as_ptr()) })
    }

    pub(crate) fn log_host(
        &self,
        host: &rsf_game_host_services,
        message: &CStr,
    ) -> Result<(), Threw> {
        self.log(host.log, host.user, message)
    }

    pub(crate) fn initialize_hooks(&self) -> bool {
        matches!(
            unsafe { MH_Initialize() },
            MH_OK | MH_ERROR_ALREADY_INITIALIZED
        )
    }

    pub(crate) fn enable_hook(&self, target: usize) -> bool {
        unsafe { MH_EnableHook(target as *mut c_void) == MH_OK }
    }

    pub(crate) fn disable_hook(&self, target: usize) -> bool {
        matches!(
            unsafe { MH_DisableHook(target as *mut c_void) },
            MH_OK | MH_ERROR_DISABLED
        )
    }

    pub(crate) fn host_create(
        &self,
        create: rsf_fallout4_host_create_fn,
        setup: &rsf_d3d11_host_setup,
        services: &mut rsf_game_host_services,
    ) -> Result<rsf_result, Threw> {
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe {
            rsf_fallout4_native_host_create(create, setup, services, &mut result)
        })?;
        Ok(result)
    }

    pub(crate) fn host_set_active(
        &self,
        activate: rsf_fallout4_host_active_fn,
        user: *mut c_void,
        active: u32,
    ) -> Result<rsf_result, Threw> {
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe {
            rsf_fallout4_native_host_set_active(activate, user, active, &mut result)
        })?;
        Ok(result)
    }

    pub(crate) fn creation_begin(
        &self,
        begin: creation::Begin,
        user: *mut c_void,
        scope: &mut rsf_d3d11_creation_scope,
    ) -> Result<rsf_result, Threw> {
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe {
            rsf_fallout4_native_creation_begin(Some(begin), user, scope, &mut result)
        })?;
        Ok(result)
    }

    pub(crate) fn creation_end(
        &self,
        end: creation::End,
        user: *mut c_void,
        scope: &rsf_d3d11_creation_scope,
        outputs: Option<&rsf_d3d11_native_chain>,
    ) -> Result<rsf_result, Threw> {
        let outputs = outputs.map_or(std::ptr::null(), std::ptr::from_ref);
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe {
            rsf_fallout4_native_creation_end(Some(end), user, scope, outputs, &mut result)
        })?;
        Ok(result)
    }

    pub(crate) fn bind_presentation(
        &self,
        bind: creation::Bind,
        user: *mut c_void,
        binding: &rsf_d3d11_presentation_binding,
    ) -> Result<rsf_result, Threw> {
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe {
            rsf_fallout4_native_bind_presentation(Some(bind), user, binding, &mut result)
        })?;
        Ok(result)
    }
}

/// Plain data that every bit pattern of its size represents.
///
/// # Safety
/// Implementors must accept any byte contents.
pub(crate) unsafe trait Plain: Copy {}
unsafe impl Plain for u8 {}
unsafe impl Plain for i32 {}
unsafe impl Plain for u32 {}
unsafe impl Plain for u64 {}
unsafe impl Plain for usize {}
unsafe impl Plain for f32 {}
unsafe impl<const N: usize> Plain for [u8; N] {}
unsafe impl<const N: usize> Plain for [f32; N] {}

/// A guarded engine read: the whole value or nothing, never a fault.
pub(crate) fn read<T: Plain>(address: usize) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut copied = 0;
    let read = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            value.as_mut_ptr().cast(),
            size_of::<T>(),
            &mut copied,
        )
    };
    // SAFETY: the read filled every byte, and T accepts any contents.
    (read != 0 && copied == size_of::<T>()).then(|| unsafe { value.assume_init() })
}

/// Compares an entire guarded instruction span within the caller's byte limit.
fn matches_bytes<const N: usize>(address: usize, expected: &[u8]) -> bool {
    let mut actual = [0u8; N];
    let Some(actual) = actual.get_mut(..expected.len()) else {
        return false;
    };
    let mut copied = 0;
    let read = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            actual.as_mut_ptr().cast(),
            actual.len(),
            &mut copied,
        )
    };
    read != 0 && copied == actual.len() && &*actual == expected
}

pub(crate) fn image() -> usize {
    unsafe { GetModuleHandleW(std::ptr::null()) as usize }
}

/// Pins the module that contains `address` for the process.
pub(crate) fn pin(address: *const c_void) -> bool {
    let mut module = null_mut();
    let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
    unsafe { GetModuleHandleExW(flags, address.cast(), &mut module) != 0 }
}

/// A terminated C string for the host log.
pub(crate) fn message(text: String) -> CString {
    let mut bytes = text.into_bytes();
    bytes.retain(|&byte| byte != 0);
    // SAFETY: retain removed every interior NUL.
    unsafe { CString::from_vec_unchecked(bytes) }
}

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    if probe.is_null() || unsafe { probe.cast::<u32>().read() } < size_of::<rsf_game_probe>() as u32
    {
        return UNKNOWN;
    }
    let probe = unsafe { &*probe };
    if probe.pe_machine != AMD64
        || probe.executable_name_utf8.is_null()
        || probe.sha256_hex.is_null()
    {
        return UNKNOWN;
    }
    let executable = unsafe { CStr::from_ptr(probe.executable_name_utf8) };
    let digest = unsafe { CStr::from_ptr(probe.sha256_hex) };
    if executable
        .to_bytes()
        .eq_ignore_ascii_case(EXECUTABLE.to_bytes())
        && digest
            .to_bytes()
            .eq_ignore_ascii_case(EXECUTABLE_SHA256.to_bytes())
    {
        RECOGNIZED
    } else {
        UNKNOWN
    }
}

/// Publishes the exported Fallout 4 GameSDK table.
///
/// # Safety
/// The output points to a caller-owned plugin table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_game_plugin_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
) -> rsf_result {
    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return RSF_ERROR_ABI_MISMATCH;
    }
    let _ = OPS.get_or_init(|| Ops);
    let api = rsf_game_plugin_api {
        struct_size: size_of::<rsf_game_plugin_api>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        info: rsf_game_info {
            id: c"fallout4".as_ptr(),
            name: c"Fallout 4".as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status: PENDING.as_ptr(),
        },
        detect: Some(detect),
        hooks: rsf_game_hooks_api {
            prepare: Some(session::prepare),
            start: Some(session::start),
            quiesce: Some(session::quiesce),
            stop: Some(session::stop),
            status: Some(session::status),
        },
    };
    unsafe { out.write(api) };
    RSF_OK
}
