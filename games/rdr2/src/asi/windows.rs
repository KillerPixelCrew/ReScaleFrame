// SPDX-License-Identifier: MIT
//! Win32 ASI admission and the engine settings call adapters.
#[path = "diagnostic_services.rs"]
mod diagnostic_services;

pub(crate) use crate::camera_probe::rsf_rdr2_install_camera_probe as camera_probe;
pub(crate) use crate::d3d12_trace::install as d3d12_trace;
use crate::runtime_worker::rsf_rdr2_asi_start_runtime;
use crate::sys::rsf_rdr2_asi_services;
use crate::sys::rsf_rdr2_runtime_leaves;
#[cfg(feature = "capture-fixture")]
pub(crate) use crate::sys::{RSF_CENSUS_ABI_VERSION, rsf_census_options};
pub(crate) use diagnostic_services::install_census as census;
use diagnostic_services::{install_frame_probe, start_diagnostics};
use rsf_minhook_sys::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleFileNameW, GetModuleHandleW,
};
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentThreadId};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW,
};

struct Startup {
    module: usize,
    services: rsf_rdr2_asi_services,
    runtime: rsf_rdr2_runtime_leaves,
}

static STARTUP: OnceLock<Startup> = OnceLock::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_asi_process_attach(
    module: HMODULE,
    services: &rsf_rdr2_asi_services,
) {
    let startup = Startup {
        module: module as usize,
        services: *services,
        runtime: rsf_rdr2_runtime_leaves {
            fingerprint: services.fingerprint,
            diagnostics: Some(start_diagnostics),
        },
    };
    if STARTUP.set(startup).is_ok() {
        unsafe {
            DisableThreadLibraryCalls(module);
            super::attach_process();
        }
    }
}

fn services() -> &'static rsf_rdr2_asi_services {
    &STARTUP
        .get()
        .expect("ASI services are registered before startup")
        .services
}

pub(crate) unsafe fn native_capture(directory: *const u16) -> bool {
    services()
        .native_capture
        .is_some_and(|capture| unsafe { capture(directory) })
}

/// The frame probe, when the ASI linked RenderDoc support.
pub(crate) fn frame_probe() -> Option<unsafe extern "C" fn(*const u16, *const u16) -> i32> {
    let services = services();
    services
        .frame_initialise
        .zip(services.frame_trigger)
        .map(|_| install_frame_probe as _)
}

pub(crate) unsafe extern "C" fn camera_reference(path: *const u16) -> bool {
    let Some(startup) = STARTUP.get() else {
        return false;
    };
    unsafe {
        crate::camera_probe::rsf_rdr2_install_camera_reference(startup.module as HMODULE, path)
    }
}

pub(crate) unsafe extern "C" fn social_club(trace: bool) -> bool {
    services()
        .social_current_index
        .is_some_and(|current_index| crate::social_club::install(current_index, trace))
}

pub(crate) unsafe extern "C" fn start_worker() {
    let thread = unsafe { CreateThread(null(), 0, Some(worker), null_mut(), 0, null_mut()) };
    if !thread.is_null() {
        unsafe { CloseHandle(thread) };
    }
}

unsafe extern "system" fn worker(_: *mut c_void) -> u32 {
    let Some(startup) = STARTUP.get() else {
        return 1;
    };
    unsafe { rsf_rdr2_asi_start_runtime(startup.module as HMODULE, &startup.runtime) }
}

pub(crate) unsafe extern "C" fn main_module() -> usize {
    unsafe { GetModuleHandleW(null()) as usize }
}

pub(crate) unsafe extern "C" fn adapter_vendor(index: u32) -> u32 {
    unsafe {
        CreateDXGIFactory1::<IDXGIFactory1>()
            .and_then(|factory| factory.EnumAdapters1(index))
            .and_then(|adapter| adapter.GetDesc1())
            .map_or(0, |description| description.VendorId)
    }
}

pub(crate) unsafe extern "C" fn executable_path(out: *mut u16, capacity: u32) -> u32 {
    unsafe { GetModuleFileNameW(null_mut(), out, capacity) }
}

pub(crate) unsafe extern "C" fn module_path(out: *mut u16, capacity: u32) -> u32 {
    let Some(startup) = STARTUP.get() else {
        return 0;
    };
    unsafe { GetModuleFileNameW(startup.module as HMODULE, out, capacity) }
}

pub(crate) unsafe extern "C" fn current_thread() -> u32 {
    unsafe { GetCurrentThreadId() }
}

pub(crate) unsafe extern "C" fn profile_int(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    fallback: i32,
) -> u32 {
    unsafe { GetPrivateProfileIntW(section, key, fallback, path) as u32 }
}

pub(crate) unsafe extern "C" fn profile_string(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    out: *mut u16,
    capacity: u32,
) -> u32 {
    unsafe {
        GetPrivateProfileStringW(section, key, windows_sys::core::w!(""), out, capacity, path)
    }
}

pub(crate) unsafe extern "C" fn hook_initialize() -> i32 {
    unsafe { MH_Initialize() }
}

pub(crate) unsafe extern "C" fn hook_create(
    target: usize,
    detour: *const c_void,
    original: *mut *mut c_void,
) -> i32 {
    unsafe { MH_CreateHook(target as _, detour.cast_mut(), original) }
}

pub(crate) unsafe extern "C" fn hook_queue_enable(target: usize) -> i32 {
    unsafe { MH_QueueEnableHook(target as _) }
}

pub(crate) unsafe extern "C" fn hook_apply_queued() -> i32 {
    unsafe { MH_ApplyQueued() }
}

pub(crate) unsafe extern "C" fn hook_disable(target: usize) -> i32 {
    unsafe { MH_DisableHook(target as _) }
}

pub(crate) unsafe extern "C" fn hook_remove(target: usize) -> i32 {
    unsafe { MH_RemoveHook(target as _) }
}

pub(crate) unsafe extern "C" fn desktop_refresh() -> u32 {
    let mut desktop = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    if unsafe { EnumDisplaySettingsW(null(), ENUM_CURRENT_SETTINGS, &mut desktop) } == 0 {
        return 0;
    }
    desktop.dmDisplayFrequency
}

pub(crate) unsafe extern "C" fn engine_manager(function: usize) -> *mut c_void {
    let function: unsafe extern "C" fn() -> *mut c_void = unsafe { std::mem::transmute(function) };
    unsafe { function() }
}

pub(crate) unsafe extern "C" fn engine_owner(function: usize, manager: *mut c_void) -> *mut c_void {
    let function: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        unsafe { std::mem::transmute(function) };
    unsafe { function(manager) }
}

#[repr(C)]
struct SettingsPath {
    ids: *const u32,
    count: u64,
}

pub(crate) unsafe extern "C" fn engine_find(
    function: usize,
    manager: *mut c_void,
    ids: *const u32,
    count: u64,
    scratch: *mut c_void,
) -> *mut c_void {
    let path = SettingsPath { ids, count };
    let function: unsafe extern "C" fn(
        *mut c_void,
        *const SettingsPath,
        *mut c_void,
    ) -> *mut c_void = unsafe { std::mem::transmute(function) };
    unsafe { function(manager, &path, scratch) }
}
