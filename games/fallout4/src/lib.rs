// SPDX-License-Identifier: MIT
//! Fallout 4 AE 1.11.240 adapter policy: the exact recognized build, the GameSDK table, the plugin
//! lifecycle with its process-lived Runtime session, DPI startup, the cold native D3D11
//! creation and presentation binding sequence, the CPU event owner (poll and worker source
//! handoff, input receipts and their GameSDK facts), native depth-domain admission and
//! composition, native input/output resolution scopes, World/Present admission, and render-input
//! admission with its camera history and leased passes.
//!
//! Native leaves stay in C++ and reach this crate through rsf_fallout4_platform
//! (fallout4_platform.h), rsf_fallout4_cpu_platform (cpu_events_platform.h) and the per-call
//! depth-domain and render-input leaves (depth_domains_platform.h, render_inputs_platform.h):
//! MinHook, the typed detour shells with their return addresses and trampoline calls, exception
//! guards, the shared file hash, the Steam chain facade, the render subsystem, the typed COM
//! leases, and the D3D11 and COM facts.
//! The Runtime decides vendors, presentation and pacing; this crate reports native facts only.
#![cfg(windows)]

mod camera;
mod cpu_abi;
mod cpu_events;
mod creation;
mod depth_domains;
mod dpi;
mod platform_abi;
mod render_hooks;
mod render_inputs;
mod resolution;
mod screen;
mod session;

use platform_abi::{
    rsf_d3d11_creation_scope, rsf_d3d11_host_setup, rsf_d3d11_native_chain,
    rsf_d3d11_presentation_binding, rsf_fallout4_bind_presentation_fn,
    rsf_fallout4_creation_begin_fn, rsf_fallout4_creation_end_fn, rsf_fallout4_host_active_fn,
    rsf_fallout4_host_create_fn, rsf_fallout4_platform,
};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_hooks_api, rsf_game_host_services, rsf_game_info,
    rsf_game_log_fn, rsf_game_plugin_api, rsf_game_probe, rsf_result,
};
use std::ffi::{CStr, c_void};
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

/// The validated native leaf table. Every entry is present.
pub(crate) struct Ops(rsf_fallout4_platform);

static OPS: OnceLock<Ops> = OnceLock::new();

pub(crate) fn ops() -> Option<&'static Ops> {
    OPS.get()
}

fn completed(returned: bool) -> Result<(), Threw> {
    if returned { Ok(()) } else { Err(Threw) }
}

impl Ops {
    fn complete(table: &rsf_fallout4_platform) -> bool {
        table.log.is_some()
            && table.file_sha256.is_some()
            && table.prepare_steam_chain.is_some()
            && table.initialize_hooks.is_some()
            && table.create_creation_hook.is_some()
            && table.enable_hook.is_some()
            && table.disable_hook.is_some()
            && table.call_create.is_some()
            && table.host_create.is_some()
            && table.host_set_active.is_some()
            && table.creation_begin.is_some()
            && table.creation_end.is_some()
            && table.bind_presentation.is_some()
            && table.subsystem.is_some()
    }

    pub(crate) fn log(
        &self,
        log: rsf_game_log_fn,
        user: *mut c_void,
        message: &CStr,
    ) -> Result<(), Threw> {
        let leaf = self.0.log.ok_or(Threw)?;
        completed(unsafe { leaf(log, user, message.as_ptr()) })
    }

    pub(crate) fn log_host(
        &self,
        host: &rsf_game_host_services,
        message: &CStr,
    ) -> Result<(), Threw> {
        self.log(host.log, host.user, message)
    }

    /// `path` is a terminated wide path.
    pub(crate) fn file_sha256(&self, path: &[u16]) -> Result<Option<[u8; 64]>, Threw> {
        let leaf = self.0.file_sha256.ok_or(Threw)?;
        let mut digest = [0u8; 64];
        let mut hashed = false;
        completed(unsafe { leaf(path.as_ptr(), digest.as_mut_ptr().cast(), &mut hashed) })?;
        Ok(hashed.then_some(digest))
    }

    pub(crate) fn prepare_steam_chain(&self, host: &rsf_game_host_services) -> bool {
        self.0
            .prepare_steam_chain
            .is_some_and(|leaf| unsafe { leaf(host) })
    }

    pub(crate) fn initialize_hooks(&self) -> bool {
        self.0
            .initialize_hooks
            .is_some_and(|leaf| unsafe { leaf() })
    }

    pub(crate) fn create_creation_hook(&self, target: usize) -> bool {
        self.0
            .create_creation_hook
            .is_some_and(|leaf| unsafe { leaf(target as *mut c_void) })
    }

    pub(crate) fn enable_hook(&self, target: usize) -> bool {
        self.0
            .enable_hook
            .is_some_and(|leaf| unsafe { leaf(target as *mut c_void) })
    }

    pub(crate) fn disable_hook(&self, target: usize) -> bool {
        self.0
            .disable_hook
            .is_some_and(|leaf| unsafe { leaf(target as *mut c_void) })
    }

    pub(crate) fn call_create(&self, call: &platform_abi::rsf_fallout4_create_call) -> i32 {
        match self.0.call_create {
            Some(leaf) => unsafe { leaf(call) },
            None => creation::E_FAIL,
        }
    }

    pub(crate) fn host_create(
        &self,
        create: rsf_fallout4_host_create_fn,
        setup: &rsf_d3d11_host_setup,
        services: &mut rsf_game_host_services,
    ) -> Result<rsf_result, Threw> {
        let leaf = self.0.host_create.ok_or(Threw)?;
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe { leaf(create, setup, services, &mut result) })?;
        Ok(result)
    }

    pub(crate) fn host_set_active(
        &self,
        activate: rsf_fallout4_host_active_fn,
        user: *mut c_void,
        active: u32,
    ) -> Result<rsf_result, Threw> {
        let leaf = self.0.host_set_active.ok_or(Threw)?;
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe { leaf(activate, user, active, &mut result) })?;
        Ok(result)
    }

    pub(crate) fn creation_begin(
        &self,
        begin: rsf_fallout4_creation_begin_fn,
        user: *mut c_void,
        scope: &mut rsf_d3d11_creation_scope,
    ) -> Result<rsf_result, Threw> {
        let leaf = self.0.creation_begin.ok_or(Threw)?;
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe { leaf(begin, user, scope, &mut result) })?;
        Ok(result)
    }

    pub(crate) fn creation_end(
        &self,
        end: rsf_fallout4_creation_end_fn,
        user: *mut c_void,
        scope: &rsf_d3d11_creation_scope,
        outputs: Option<&rsf_d3d11_native_chain>,
    ) -> Result<rsf_result, Threw> {
        let leaf = self.0.creation_end.ok_or(Threw)?;
        let outputs = outputs.map_or(std::ptr::null(), std::ptr::from_ref);
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe { leaf(end, user, scope, outputs, &mut result) })?;
        Ok(result)
    }

    pub(crate) fn bind_presentation(
        &self,
        bind: rsf_fallout4_bind_presentation_fn,
        user: *mut c_void,
        binding: &rsf_d3d11_presentation_binding,
    ) -> Result<rsf_result, Threw> {
        let leaf = self.0.bind_presentation.ok_or(Threw)?;
        let mut result = RSF_ERROR_NOT_READY;
        completed(unsafe { leaf(bind, user, binding, &mut result) })?;
        Ok(result)
    }

    /// One RSF_FALLOUT4_* subsystem operation. `services` is read only by the prepare operations.
    pub(crate) fn subsystem(
        &self,
        operation: u32,
        services: Option<&rsf_game_host_services>,
    ) -> Result<bool, Threw> {
        let leaf = self.0.subsystem.ok_or(Threw)?;
        let services = services.map_or(std::ptr::null(), std::ptr::from_ref);
        let mut value = false;
        completed(unsafe { leaf(operation, services, &mut value) })?;
        Ok(value)
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
pub(crate) fn message(text: String) -> Vec<u8> {
    let mut bytes = text.into_bytes();
    bytes.retain(|&byte| byte != 0);
    bytes.push(0);
    bytes
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

/// Publishes the GameSDK table for the exported rsf_get_game_plugin_api.
///
/// # Safety
/// `out` is a caller-owned plugin table. `platform` must be a complete table that lives for the
/// process.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
    platform: *const rsf_fallout4_platform,
) -> rsf_result {
    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return RSF_ERROR_ABI_MISMATCH;
    }
    let Some(table) = (unsafe { platform.as_ref() }) else {
        return RSF_ERROR_NOT_READY;
    };
    if !Ops::complete(table) {
        return RSF_ERROR_NOT_READY;
    }
    let _ = OPS.get_or_init(|| Ops(*table));
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
