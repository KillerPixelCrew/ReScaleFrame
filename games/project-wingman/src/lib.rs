// SPDX-License-Identifier: MIT
//! Project Wingman game SDK entry. Recognized by executable name; prepare discovers whether the
//! running build is usable.
mod camera;
mod discovery;
mod execution;
mod observer;
#[cfg(windows)]
mod windows;

use rsf_game_sys::session::{Probe, publish_api};
use rsf_game_sys::{
    rsf_detection, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api, rsf_game_probe,
    rsf_result,
};
use std::ffi::CStr;

use rsf_game_sys::RSF_ERROR_INVALID_ARGUMENT as INVALID_ARGUMENT;
const EXECUTABLE: &CStr = c"ProjectWingman-Win64-Shipping.exe";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    unsafe {
        Probe::detect(probe, |probe| {
            probe
                .executable
                .to_bytes()
                .eq_ignore_ascii_case(EXECUTABLE.to_bytes())
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
) -> rsf_result {
    let info = rsf_game_info {
        id: c"project-wingman".as_ptr(),
        name: c"Project Wingman".as_ptr(),
        version: VERSION.as_ptr().cast(),
        rendering_ready: 0,
        status: c"Native temporal GPU input leases; SR, FG, latency and VR processing are unimplemented."
            .as_ptr(),
    };
    let hooks = rsf_game_hooks_api {
        prepare: Some(observer::prepare),
        start: Some(observer::start),
        quiesce: Some(observer::quiesce),
        stop: Some(observer::stop),
        status: Some(observer::status),
    };
    unsafe { publish_api(requested_abi, out, info, detect, hooks) }
}
