// SPDX-License-Identifier: MIT
//! The ASI's C link services, startup worker thread and the engine settings call adapters.
#[cfg(feature = "research")]
#[path = "diagnostic_services.rs"]
mod diagnostic_services;

#[cfg(feature = "research")]
pub(crate) use crate::camera_probe::rsf_rdr2_install_camera_probe as camera_probe;
#[cfg(feature = "research")]
pub(crate) use crate::d3d12_trace::install as d3d12_trace;
use crate::runtime_worker::asi_start_runtime;
use crate::sys::rsf_rdr2_asi_services;
#[cfg(all(feature = "capture-fixture", not(test)))]
pub(crate) use crate::sys::{RSF_CENSUS_ABI_VERSION, rsf_census_options};
#[cfg(feature = "research")]
pub(crate) use diagnostic_services::install_census as census;
#[cfg(feature = "research")]
use diagnostic_services::install_frame_probe;
#[cfg(feature = "research")]
pub(crate) use diagnostic_services::{census_report, dump, measure, module_list};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::Threading::CreateThread;

struct Startup {
    module: usize,
    services: rsf_rdr2_asi_services,
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
    };
    if STARTUP.set(startup).is_ok() {
        unsafe {
            DisableThreadLibraryCalls(module);
        }
        super::attach(module);
    }
}

fn services() -> &'static rsf_rdr2_asi_services {
    &STARTUP
        .get()
        .expect("ASI services are registered before startup")
        .services
}

#[cfg(feature = "research")]
pub(crate) unsafe fn native_capture(directory: *const u16) -> bool {
    services()
        .native_capture
        .is_some_and(|capture| unsafe { capture(directory) })
}

/// The frame probe, when the ASI linked RenderDoc support.
#[cfg(feature = "research")]
pub(crate) fn frame_probe() -> Option<unsafe extern "C" fn(*const u16, *const u16) -> i32> {
    let services = services();
    services
        .frame_initialise
        .zip(services.frame_trigger)
        .map(|_| install_frame_probe as _)
}

#[cfg(feature = "research")]
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
    let Some(fingerprint) = startup.services.fingerprint else {
        return 1;
    };
    asi_start_runtime(startup.module as HMODULE, fingerprint)
}

pub(crate) unsafe extern "C" fn adapter_vendor(index: u32) -> u32 {
    unsafe {
        CreateDXGIFactory1::<IDXGIFactory1>()
            .and_then(|factory| factory.EnumAdapters1(index))
            .and_then(|adapter| adapter.GetDesc1())
            .map_or(0, |description| description.VendorId)
    }
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
