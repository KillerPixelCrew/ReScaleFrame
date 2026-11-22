// SPDX-License-Identifier: MIT
//! Red Dead Redemption 2 (Steam 1.0.1491.50) game plugin policy for `ReScaleFrame.Game.RDR2.dll`.
//! Recognition, lifecycle validation, native SR routing and the completed-scene producer live here.
//! Windows and COM services are implemented in the platform module.
//!
//! `ReScaleFrame.RDR2.asi` links the same archive for its startup admission and engine-settings
//! ownership (`asi`, `engine_settings`). The ASI supplies its native diagnostic callbacks.
mod asi;
mod camera;
#[cfg(windows)]
mod camera_probe;
mod capture_state;
#[cfg(all(windows, not(test)))]
mod d3d12_trace;
#[cfg(windows)]
mod diagnostics;
mod engine_settings;
mod host;
mod layout;
#[cfg(windows)]
mod native_settings;
mod platform;
mod renderer;
#[cfg(windows)]
mod runtime_worker;
#[cfg(windows)]
mod runtime_worker_abi;
mod scene;
#[cfg(windows)]
mod script_input;
#[cfg(windows)]
mod script_input_abi;
#[cfg(windows)]
mod social_club;

use platform::Platform;
use rsf_game_sys::session::Probe;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_game_control_args, rsf_game_hooks_api, rsf_game_info,
    rsf_game_plugin_api, rsf_game_prepare_args, rsf_game_probe, rsf_game_renderer_status,
    rsf_game_start_args,
};
use std::ffi::CStr;

/// rsf_result values from game_api.h.
pub(crate) const OK: i32 = 0;
pub(crate) const INVALID_ARGUMENT: i32 = -1;
pub(crate) const ABI_MISMATCH: i32 = -2;
pub(crate) const NOT_READY: i32 = -3;
pub(crate) const BUSY: i32 = -4;
pub(crate) const NATIVE_REFUSED: i32 = -5;
/// rsf_detection values.
const UNKNOWN: u32 = 0;
const RECOGNIZED: u32 = 1;
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const STATUS: &CStr =
    c"Native DLSS SR replacement implemented; game acceptance, FSR routing and FG pending.";

/// Size and ABI check for a call structure that leads with `struct_size` and `abi_version`.
///
/// # Safety
/// `value` is null or points at a readable structure of at least its declared size.
unsafe fn validate<T>(value: *const T) -> i32 {
    if value.is_null() {
        return INVALID_ARGUMENT;
    }
    // Only the size field is readable from a short SDK structure.
    let header = value.cast::<u32>();
    if unsafe { header.read() } < size_of::<T>() as u32 {
        return INVALID_ARGUMENT;
    }
    if unsafe { header.add(1).read() } == RSF_GAME_ABI_VERSION {
        OK
    } else {
        ABI_MISMATCH
    }
}

unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> i32 {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let host = unsafe { (*args).host };
    let valid = unsafe { validate(host) };
    if valid != OK {
        return valid;
    }
    // The orchestrator lends the services for the call; the renderer keeps a copy.
    let host = unsafe { *host };
    if host.session_id == 0 {
        return INVALID_ARGUMENT;
    }
    renderer::prepare(&host)
}

unsafe extern "C" fn start(args: *const rsf_game_start_args) -> i32 {
    match unsafe { validate(args) } {
        OK => renderer::start(),
        refused => refused,
    }
}

unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> i32 {
    match unsafe { validate(args) } {
        OK => renderer::quiesce(),
        refused => refused,
    }
}

unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> i32 {
    match unsafe { validate(args) } {
        OK => renderer::stop(),
        refused => refused,
    }
}

unsafe extern "C" fn status(output: *mut rsf_game_renderer_status) -> i32 {
    let valid = unsafe { validate(output.cast_const()) };
    if valid == OK {
        renderer::status(unsafe { &mut *output });
    }
    valid
}

unsafe extern "C" fn detect(raw: *const rsf_game_probe) -> u32 {
    if unsafe { Probe::from_raw(raw) }.is_some_and(|probe| {
        probe.machine == layout::AMD64 && probe.matches(layout::EXECUTABLE_NAME, layout::SHA256)
    }) {
        RECOGNIZED
    } else {
        UNKNOWN
    }
}

/// Public entry exported by the game plugin's module definition.
#[cfg(all(windows, not(test)))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_game_plugin_api(
    requested_abi: u32,
    api: *mut rsf_game_plugin_api,
) -> i32 {
    unsafe { rsf_rdr2_publish_api(requested_abi, api, platform::native()) }
}

/// Fills the plugin API for `rsf_get_game_plugin_api` and accepts the native leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
    platform: *const Platform,
) -> i32 {
    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    platform::register(platform);
    let api = rsf_game_plugin_api {
        struct_size: size_of::<rsf_game_plugin_api>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        info: rsf_game_info {
            id: c"rdr2".as_ptr(),
            name: c"Red Dead Redemption 2".as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status: STATUS.as_ptr(),
        },
        detect: Some(detect),
        hooks: rsf_game_hooks_api {
            prepare: Some(prepare),
            start: Some(start),
            quiesce: Some(quiesce),
            stop: Some(stop),
            status: Some(status),
        },
    };
    unsafe { out.write(api) };
    OK
}

#[cfg(test)]
mod tests;
