// SPDX-License-Identifier: MIT
#[cfg(windows)]
mod adapter_options;
#[cfg(windows)]
mod bootstrap;
#[cfg_attr(not(windows), allow(dead_code))]
mod bootstrap_abi;
mod camera;
#[cfg(windows)]
mod capture_clock;
#[cfg(windows)]
mod digest;
mod engine_guards;
#[cfg(windows)]
mod engine_slots;
#[cfg(windows)]
mod executable;
mod ffi;
#[cfg(any(windows, test))]
mod fg_inputs;
#[cfg(any(windows, test))]
mod fg_policy;
#[cfg(windows)]
mod fg_stage;
#[cfg(windows)]
mod fg_stage_abi;
#[cfg(all(windows, any(not(feature = "native-entry"), feature = "sr-fixtures")))]
mod fixture_profile;
mod frame_dump;
mod geometry_registry;
mod geometry_registry_abi;
#[cfg(windows)]
mod host_log;
#[cfg(windows)]
mod ini;
mod input;
mod lease;
mod motion;
mod motion_device;
mod motion_device_abi;
#[cfg(windows)]
mod motion_frame_target_abi;
mod motion_target;
mod motion_target_abi;
mod native_geometry;
mod native_geometry_abi;
#[cfg(any(windows, test))]
mod native_hooks;
#[cfg(windows)]
mod native_observer;
#[cfg(windows)]
mod native_observer_abi;
#[cfg(windows)]
mod native_profile;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod overlay_abi;
#[cfg(windows)]
mod overlay_hooks;
#[cfg(windows)]
mod overlay_native;
#[cfg(all(windows, feature = "native-entry"))]
mod plugin_entry;
#[cfg(windows)]
mod runtime_module;
mod shader_family;
#[cfg(windows)]
mod shader_loader;
mod sr_d3d11;
#[cfg(all(windows, feature = "sr-fixtures"))]
mod sr_fixture_device;
#[cfg(windows)]
mod sr_fixture_readback;
#[path = "../../../tests/anomaly_sr_jitter_fixture.rs"]
mod sr_jitter_fixture;
mod sr_pass;
mod sr_profile;
mod sr_reconstruction;
mod sr_stage;
mod sr_stage_abi;
mod sr_swap;
mod sr_swap_abi;
mod sr_temporal;
mod sr_tonemap;
#[cfg(all(windows, feature = "sr-fixtures"))]
mod temporal_fixture;
mod wide;

#[cfg(all(windows, feature = "native-entry"))]
use rsf_game_sys::rsf_game_probe;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detect_game_fn, rsf_game_hooks_api, rsf_game_info,
    rsf_game_plugin_api,
};
#[cfg(all(windows, feature = "native-entry"))]
use std::ffi::CStr;

#[cfg(all(windows, feature = "native-entry"))]
const UNKNOWN: u32 = 0;
#[cfg(all(windows, feature = "native-entry"))]
const RECOGNIZED: u32 = 1;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const AMD64: u16 = 0x8664;
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

#[cfg(all(windows, feature = "native-entry"))]
unsafe fn rsf_anomaly_detect(probe: *const rsf_game_probe) -> u32 {
    // Only the size field is readable from a short SDK structure.
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
    let sha256 = unsafe { CStr::from_ptr(probe.sha256_hex) };
    if native_profile::find(executable.to_bytes(), sha256.to_bytes()).is_some() {
        RECOGNIZED
    } else {
        UNKNOWN
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
    detect: rsf_detect_game_fn,
    hooks: &rsf_game_hooks_api,
) -> i32 {
    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let api = rsf_game_plugin_api {
        struct_size: size_of::<rsf_game_plugin_api>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        info: rsf_game_info {
            id: c"anomaly".as_ptr(),
            name: c"STALKER Anomaly".as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status:
                c"Guarded native boundary observation; temporal SR/FG execution is not validated."
                    .as_ptr(),
        },
        detect,
        hooks: *hooks,
    };
    unsafe { out.write(api) };
    0
}
