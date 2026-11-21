// SPDX-License-Identifier: MIT
//! Project Wingman game SDK entry and exact-build identity.
mod camera;

use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api,
    rsf_game_probe, rsf_result,
};
use std::ffi::{CStr, c_char};

const UNKNOWN: rsf_detection = 0;
const RECOGNIZED: rsf_detection = 1;
const INVALID_ARGUMENT: rsf_result = -1;
const ABI_MISMATCH: rsf_result = -2;
const AMD64: u16 = 0x8664;
const EXECUTABLE: &CStr = c"ProjectWingman-Win64-Shipping.exe";
const SHA256: &CStr = c"092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

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
        && digest.to_bytes().eq_ignore_ascii_case(SHA256.to_bytes())
    {
        RECOGNIZED
    } else {
        UNKNOWN
    }
}

// The native observer checks the loaded module again before installing its hooks.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_matches_executable(name: *const u16) -> bool {
    if name.is_null() {
        return false;
    }
    for (i, expected) in EXECUTABLE.to_bytes_with_nul().iter().enumerate() {
        // A shorter string mismatches at its terminator before the next read.
        let value = unsafe { name.add(i).read() };
        if !u8::try_from(value).is_ok_and(|value| value.eq_ignore_ascii_case(expected)) {
            return false;
        }
    }
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_matches_sha256(digest: *const c_char) -> bool {
    !digest.is_null() && unsafe { CStr::from_ptr(digest) } == SHA256
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
    hooks: &rsf_game_hooks_api,
) -> rsf_result {
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
            id: c"project-wingman".as_ptr(),
            name: c"Project Wingman".as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status: c"Native temporal GPU input leases; SR, FG, latency and VR processing are unimplemented."
                .as_ptr(),
        },
        detect: Some(detect),
        hooks: *hooks,
    };
    unsafe { out.write(api) };
    0
}
