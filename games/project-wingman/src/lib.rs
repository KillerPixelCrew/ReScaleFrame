// SPDX-License-Identifier: MIT
//! Project Wingman game SDK entry and exact-build identity.
mod camera;
mod execution;
mod observer;
#[cfg(windows)]
mod windows;

use rsf_game_sys::session::Probe;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api,
    rsf_game_probe, rsf_result,
};
use std::ffi::CStr;

use rsf_game_sys::{
    RSF_ERROR_ABI_MISMATCH as ABI_MISMATCH, RSF_ERROR_INVALID_ARGUMENT as INVALID_ARGUMENT,
    RSF_GAME_RECOGNIZED as RECOGNIZED, RSF_GAME_UNKNOWN as UNKNOWN,
};
const AMD64: u16 = 0x8664;
const EXECUTABLE: &CStr = c"ProjectWingman-Win64-Shipping.exe";
const SHA256: &CStr = c"092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    if unsafe { Probe::from_raw(probe) }
        .is_some_and(|probe| probe.machine == AMD64 && probe.matches(EXECUTABLE, SHA256))
    {
        RECOGNIZED
    } else {
        UNKNOWN
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
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
        hooks: rsf_game_hooks_api {
            prepare: Some(observer::prepare),
            start: Some(observer::start),
            quiesce: Some(observer::quiesce),
            stop: Some(observer::stop),
            status: Some(observer::status),
        },
    };
    unsafe { out.write(api) };
    0
}
