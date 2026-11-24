// SPDX-License-Identifier: MIT
//! The adapter's stage calls for the cold start (`bootstrap::leaves`): the C entries of the stage leaves (sr_stage_platform.c,
//! motion_stage_platform.c, fg_stage_platform.c, bootstrap_platform.c, native_hooks_platform.c) and the Rust stages.
use super::*;
use crate::engine_guards::{Image, Read};
use crate::engine_slots::Exchange;
use crate::executable::running_build;
use crate::fg_stage::rsf_anomaly_fg_stage_input_received;
pub(super) use crate::adapter_options::register as register_options;
pub(super) use crate::fg_stage::rsf_anomaly_fg_installed as fg_installed;
use crate::fg_stage::rsf_anomaly_fg_set_overlay_sink;
use crate::input::rsf_anomaly_input_install;
use crate::input::rsf_anomaly_input_received;
use crate::native_hooks::HookOps;
use crate::native_profile::PROFILES;
use crate::overlay::{rsf_anomaly_overlay_present, rsf_anomaly_overlay_start};
use crate::overlay_hooks::{
    rsf_anomaly_overlay_install_default_input, rsf_anomaly_overlay_set_input_observer,
};
use crate::sr_stage::native::rsf_anomaly_sr_depth_rva;
use crate::sys::rsf_anomaly_fg_swap_chain;
use crate::sys::{
    RSF_ANOMALY_OVERLAY_ALREADY_STARTED, RSF_ANOMALY_OVERLAY_STARTED, rsf_anomaly_overlay_options,
};
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::OsString;
use std::ffi::{c_char, c_void};
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
};
use windows_sys::core::w;

// rsf_anomaly_start refusals that only the entry reports.
const CONFIGURATION_REFUSED: u32 = 1;
const BUILD_REFUSED: u32 = 2;
const RUNTIME_MISSING: u32 = 3;
const MODE_REFUSED: u32 = 7;
const FAULT: u32 = 8;

pub(super) use self::rsf_anomaly_bootstrap_input_hook as input_hook;
pub(super) use self::rsf_anomaly_motion_configure_native as motion;
pub(super) use self::rsf_anomaly_sr_configure_native as sr;

unsafe extern "C" {
    pub(super) fn rsf_anomaly_sr_configure_native(
        ini: *const u16,
        apply: bool,
        log: rsf_game_log_fn,
    ) -> bool;
    pub(super) fn rsf_anomaly_motion_configure_native(ini: *const u16, log: rsf_game_log_fn);
    fn rsf_anomaly_fg_configure_native(
        ini: *const u16,
        apply: bool,
        runtime: HMODULE,
        depth_rva: u32,
        log: rsf_game_log_fn,
    ) -> bool;
    /// Installs the CInput detour (its unwind boundary is C) through `rsf_anomaly_bootstrap_install_input`.
    pub(super) fn rsf_anomaly_bootstrap_input_hook(
        image: HMODULE,
        build: usize,
        reason: *mut *const c_char,
    ) -> bool;
    fn rsf_anomaly_minhook_ops() -> &'static HookOps;
}

pub(super) unsafe fn fg(
    ini: *const u16,
    apply: bool,
    runtime: HMODULE,
    build: usize,
    log: rsf_game_log_fn,
) -> bool {
    unsafe {
        rsf_anomaly_fg_configure_native(ini, apply, runtime, rsf_anomaly_sr_depth_rva(build), log)
    }
}

pub(super) unsafe fn input_observation(reason: *mut *const c_char) -> bool {
    unsafe { rsf_anomaly_overlay_install_default_input(rsf_anomaly_minhook_ops(), &mut *reason) }
}

pub(super) unsafe fn overlay(runtime: HMODULE, ini: *const u16, log: rsf_game_log_fn) -> bool {
    unsafe { start_overlay(runtime, ini, log, rsf_anomaly_minhook_ops()) }
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_bootstrap_install_input(
    image: *mut c_void,
    build: usize,
    replacement: *mut c_void,
    exchange: Exchange,
    copy: Read,
    reason: Option<&mut *const c_char>,
) -> bool {
    let Some(profile) = PROFILES.get(build) else {
        if let Some(reason) = reason {
            *reason = c"unknown CInput build".as_ptr();
        }
        return false;
    };
    let image = Image::new(image.cast(), profile.image_size, copy);
    {
        rsf_anomaly_input_install(
            &image,
            build,
            profile.device_frame_rva,
            replacement,
            exchange,
            reason,
        )
    }
}

unsafe extern "C" fn input_received(kind: u8) {
    rsf_anomaly_input_received(kind, rsf_anomaly_fg_stage_input_received);
}

pub(super) unsafe fn publish_input_observer() {
    rsf_anomaly_overlay_set_input_observer(Some(input_received));
}

unsafe fn start_overlay(
    runtime: *mut c_void,
    configuration: *const u16,
    log: rsf_game_log_fn,
    hooks: &HookOps,
) -> bool {
    let options = rsf_anomaly_overlay_options {
        runtime,
        configuration,
        log,
        log_user: null_mut(),
        api_override: null(),
        install_hooks: 1,
    };
    let status = unsafe { rsf_anomaly_overlay_start(&options, Some(hooks), null_mut()) };
    status == RSF_ANOMALY_OVERLAY_STARTED as u32
        || status == RSF_ANOMALY_OVERLAY_ALREADY_STARTED as u32
}

unsafe extern "C" fn overlay_present(user: *mut c_void, chain: *mut rsf_anomaly_fg_swap_chain) {
    rsf_anomaly_overlay_present(user, chain.cast());
}

pub(super) unsafe fn overlay_sink() {
    rsf_anomaly_fg_set_overlay_sink(Some(overlay_present), null_mut());
}

impl Runtime {
    fn borrow() -> Option<Self> {
        use crate::runtime_module::rsf_anomaly_runtime_module;
        use rsf_game_sys::win32::typed_export;
        let module = rsf_anomaly_runtime_module();
        if module.is_null() {
            return None;
        }
        // These exports have the C signatures declared in plugin_session.h.
        let prepare = unsafe { typed_export(module, c"rsf_plugin_session_prepare") };
        let start = unsafe { typed_export(module, c"rsf_plugin_session_start") };
        let quiesce = unsafe { typed_export(module, c"rsf_plugin_session_quiesce") };
        let stop = unsafe { typed_export(module, c"rsf_plugin_session_stop") };
        Some(Self {
            module,
            prepare,
            start,
            quiesce,
            stop,
        })
    }
}

static ENTRY: Mutex<Entry> = Mutex::new(Entry::new());

fn observe_selected(ini: &[u16]) -> bool {
    let mut mode = [0u16; 64];
    let length = unsafe { crate::ini::read_text(ini.as_ptr(), w!("Mode"), w!(""), &mut mode) };
    mode[..length.min(mode.len())]
        .iter()
        .copied()
        .eq("Observe".encode_utf16())
}

unsafe fn enter(configuration: *const u16) -> u32 {
    let Some(ini) = (unsafe { crate::ini::path(configuration) }) else {
        return CONFIGURATION_REFUSED;
    };
    let attributes = unsafe { GetFileAttributesW(ini.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES || attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return CONFIGURATION_REFUSED;
    }
    let path = PathBuf::from(OsString::from_wide(&ini[..ini.len() - 1]));
    if !path.is_absolute() {
        return CONFIGURATION_REFUSED;
    }
    let mut entry = lock(&ENTRY);
    if entry.started {
        return READY;
    }
    let Ok(build) = running_build() else {
        return BUILD_REFUSED;
    };
    *lock(&LOG) = path
        .parent()
        .map(|directory| directory.join("ReScaleFrame.Anomaly.log"));
    let profile = &PROFILES[build];
    write_log(
        format!(
            "Anomaly executable {}: {} build profile selected by exact SHA-256.",
            profile.executable.to_string_lossy(),
            profile.label.to_string_lossy()
        )
        .as_bytes(),
    );
    let observe = observe_selected(ini);
    // Overlay=1 is independent of Mode: it wires only the shared overlay, never SR or FG.
    let overlay = unsafe { crate::ini::read_int(ini.as_ptr(), w!("Overlay"), 0) } != 0;
    if !observe && !overlay {
        write_log(b"Anomaly startup refused: only explicitly selected Observe mode or Overlay=1 is currently validated; SR/FG is not implemented.");
        return MODE_REFUSED;
    }
    let Some(runtime) = Runtime::borrow() else {
        write_log(b"Anomaly refused: carrier has not loaded the existing Runtime DLL.");
        return RUNTIME_MISSING;
    };
    let request = Request {
        ini,
        build,
        observe,
        overlay,
    };
    orchestrate(&mut entry, &request, &runtime)
}

#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_anomaly_start(configuration: *const u16) -> u32 {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        enter(configuration)
    }))
    .unwrap_or_else(|_| {
        write_log(b"Anomaly startup refused after a contained panic.");
        FAULT
    })
}
