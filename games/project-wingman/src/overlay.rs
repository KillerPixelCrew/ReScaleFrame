// SPDX-License-Identifier: MIT
//! Wingman's restart-only native observer option in the shared runtime panel.
use crate::overlay_abi::*;
use std::ffi::c_void;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetWindowThreadProcessId, IsWindowVisible,
};
use windows_sys::core::{s, w};

const NOT_READY: i32 = -3;

struct Configuration {
    path: Vec<u16>,
    save: rsf_adapter_setting_write_bool_fn,
    observe_native: i32,
}

// The runtime copies the option table and borrows callback code for the process lifetime.
static CONFIG: OnceLock<Configuration> = OnceLock::new();
static STARTED: OnceLock<()> = OnceLock::new();

unsafe extern "C" fn accept_window(_: *mut c_void, window: *mut c_void) -> i32 {
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(window, &mut process) };
    i32::from(
        process == unsafe { GetCurrentProcessId() }
            && unsafe { GetAncestor(window, GA_ROOT) } == window
            && unsafe { IsWindowVisible(window) } != 0,
    )
}

unsafe extern "C" fn option_changed(_: *mut c_void, index: u32, value: i32) -> i32 {
    let Some(config) = CONFIG
        .get()
        .filter(|_| index == 0 && (0..=1).contains(&value))
    else {
        return 0;
    };
    let Some(save) = config.save else {
        return 0;
    };
    i32::from(
        unsafe {
            save(
                config.path.as_ptr(),
                w!("ProjectWingman"),
                w!("ObserveNative"),
                value as u32,
            )
        } == 0,
    )
}

pub(crate) fn read(runtime: HMODULE, path: Vec<u16>) -> Result<bool, i32> {
    if let Some(config) = CONFIG.get() {
        return Ok(config.observe_native != 0);
    }
    let read: rsf_adapter_setting_read_bool_fn = unsafe {
        std::mem::transmute(GetProcAddress(runtime, s!("rsf_adapter_setting_read_bool")))
    };
    let save: rsf_adapter_setting_write_bool_fn = unsafe {
        std::mem::transmute(GetProcAddress(
            runtime,
            s!("rsf_adapter_setting_write_bool"),
        ))
    };
    let Some(read) = read else {
        return Err(NOT_READY);
    };
    let observe_native =
        unsafe { read(path.as_ptr(), w!("ProjectWingman"), w!("ObserveNative"), 0) };
    if observe_native < 0 {
        return Err(observe_native);
    }
    let config = CONFIG.get_or_init(|| Configuration {
        path,
        save,
        observe_native,
    });
    Ok(config.observe_native != 0)
}

pub(crate) fn start(runtime: HMODULE) {
    if let Some(config) = CONFIG.get() {
        STARTED.get_or_init(|| start_once(runtime, config));
    }
}

fn start_once(runtime: HMODULE, config: &Configuration) {
    let start: rsf_start_d3d11_overlay_fn =
        unsafe { std::mem::transmute(GetProcAddress(runtime, s!("rsf_start_d3d11_overlay"))) };
    if let Some(start) = start
        && config.save.is_some()
    {
        let option = rsf_overlay_service_option {
            label_utf8: c"Observe native temporal passes".as_ptr(),
            kind: RSF_OVERLAY_SERVICE_OPTION_TOGGLE,
            value: config.observe_native,
            minimum: 0,
            maximum: 1,
            step: 1,
            divisor: 1,
            restart: 1,
        };
        let settings = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            fg_backend: RSF_FG_BACKEND_AUTO,
            generated_frames: 1,
            ..unsafe { std::mem::zeroed() }
        };
        let setup = rsf_overlay_service_setup {
            struct_size: size_of::<rsf_overlay_service_setup>() as u32,
            abi_version: RSF_OVERLAY_API_VERSION,
            identity_utf8: c"Project Wingman".as_ptr(),
            configuration_path: config.path.as_ptr(),
            configuration_section: w!("ProjectWingman"),
            defaults: settings,
            latency_choices: 1 << RSF_REFLEX_OFF,
            max_generated_frames: 1,
            log: Some(crate::carrier::host_log),
            accept_window: Some(accept_window),
            option_count: 1,
            options: &option,
            option_changed: Some(option_changed),
            ..unsafe { std::mem::zeroed() }
        };
        let started = unsafe { start(&setup) };
        crate::carrier::write_log(
            format!("Shared D3D11 overlay startup result={started}.").as_bytes(),
        );
    } else {
        crate::carrier::write_log(b"Runtime shared overlay is unavailable.");
    }
}
