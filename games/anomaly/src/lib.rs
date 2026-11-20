// SPDX-License-Identifier: MIT
#[cfg(windows)]
mod bootstrap;
mod fg_inputs;
mod fg_inputs_abi;
mod fg_policy;
mod fg_policy_abi;
mod geometry_registry;
mod geometry_registry_abi;
mod input;
mod motion;
#[cfg(windows)]
mod native_observer;
#[cfg(windows)]
mod native_observer_abi;
mod sr_temporal;

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

#[repr(C)]
struct BuildIdentity {
    executable: *const c_char,
    sha256: *const c_char,
}

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
