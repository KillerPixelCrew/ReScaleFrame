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
#[cfg(all(windows, not(feature = "native-entry")))]
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
#[cfg(windows)]
mod sr_fixture_readback;
#[path = "../../../tests/anomaly_sr_jitter_fixture.rs"]
mod sr_jitter_fixture;
mod sr_pass;
mod sr_reconstruction;
mod sr_stage;
mod sr_stage_abi;
mod sr_swap;
mod sr_swap_abi;
mod sr_temporal;
mod sr_tonemap;
mod wide;

use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detect_game_fn, rsf_game_hooks_api, rsf_game_info,
    rsf_game_plugin_api, rsf_game_probe,
};
use std::ffi::{CStr, c_char};
use std::slice;

const UNKNOWN: u32 = 0;
const RECOGNIZED: u32 = 1;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const AMD64: u16 = 0x8664;
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

use bootstrap_abi::rsf_anomaly_build_identity as BuildIdentity;

fn match_build(builds: &[BuildIdentity], executable: &[u8], sha256: &[u8]) -> Option<usize> {
    builds.iter().position(|build| {
        // The native profile table supplies immutable, NUL-terminated strings.
        let name = unsafe { CStr::from_ptr(build.executable) };
        let hash = unsafe { CStr::from_ptr(build.sha256) };
        name.to_bytes().eq_ignore_ascii_case(executable)
            && hash.to_bytes().eq_ignore_ascii_case(sha256)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_match_build(
    builds: *const BuildIdentity,
    count: usize,
    executable: *const c_char,
    executable_len: usize,
    sha256: *const c_char,
    sha256_len: usize,
) -> usize {
    if executable_len == 0 || sha256_len != 64 {
        return usize::MAX;
    }
    // This private entry borrows spans from the native adapter's build lookup.
    let builds = unsafe { slice::from_raw_parts(builds, count) };
    let executable = unsafe { slice::from_raw_parts(executable.cast(), executable_len) };
    let sha256 = unsafe { slice::from_raw_parts(sha256.cast(), sha256_len) };
    match_build(builds, executable, sha256).unwrap_or(usize::MAX)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_match_build_wide(
    builds: *const BuildIdentity,
    count: usize,
    executable: *const u16,
    executable_len: usize,
    sha256: *const c_char,
    sha256_len: usize,
) -> usize {
    let mut ascii = [0u8; 63];
    if !(1..=ascii.len()).contains(&executable_len) || sha256_len != 64 {
        return usize::MAX;
    }
    let executable = unsafe { slice::from_raw_parts(executable, executable_len) };
    for (&unit, output) in executable.iter().zip(&mut ascii) {
        if !(1..=0x7f).contains(&unit) {
            return usize::MAX;
        }
        *output = unit as u8;
    }
    unsafe {
        rsf_anomaly_match_build(
            builds,
            count,
            ascii.as_ptr().cast(),
            executable_len,
            sha256,
            sha256_len,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_detect(
    probe: *const rsf_game_probe,
    builds: *const BuildIdentity,
    count: usize,
) -> u32 {
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
    let builds = unsafe { slice::from_raw_parts(builds, count) };
    if match_build(builds, executable.to_bytes(), sha256.to_bytes()).is_some() {
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
