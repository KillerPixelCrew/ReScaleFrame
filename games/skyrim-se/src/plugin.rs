// SPDX-License-Identifier: MIT
//! GameSDK identity and the exact-build admission of the native observers.
use crate::layout::{END, JITTER, PROCESS, PROCESS_CALL, RESET, RESOLUTION, UPLOAD, VIEWPORT};
use crate::platform;
use crate::session;
use rsf_game_sys::session::{Probe, publish_api};
use rsf_game_sys::win32::module_path;
use rsf_game_sys::windows_path::leaf_is;
use rsf_game_sys::{
    rsf_detection, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api, rsf_game_probe,
    rsf_result,
};
use std::ffi::CStr;

pub(crate) use rsf_game_sys::RSF_ERROR_INVALID_ARGUMENT as INVALID_ARGUMENT;
const EXECUTABLE: &CStr = c"SkyrimSE.exe";
pub(crate) const SHA256: &CStr =
    c"846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const STATUS: &CStr =
    c"Experimental native-resolution SR; reduced-resolution SR, FG and latency remain incomplete.";

/// The public lifecycle serializes observer admission and installation.
pub(crate) fn admit_observers() -> i32 {
    let Some(path) = (unsafe { module_path(std::ptr::null_mut()) }) else {
        return 0;
    };
    if !leaf_is(path.units(), EXECUTABLE) {
        return 0;
    }
    let Some(platform) = platform::get() else {
        return 0;
    };
    let base = platform.main_module();
    if base == 0
        || platform
            .module_sha256(base)
            .as_ref()
            .map(|digest| digest.as_slice())
            != Some(SHA256.to_bytes())
        || !platform.matches(base.wrapping_add(JITTER.rva), JITTER.bytes)
    {
        return 0;
    }
    if !crate::sampling::sampling_guard() || !crate::presentation::presentation_guard() {
        return 0;
    }
    if ![
        PROCESS,
        PROCESS_CALL,
        RESET,
        RESOLUTION,
        END,
        VIEWPORT,
        UPLOAD,
    ]
    .into_iter()
    .all(|site| platform.matches(base.wrapping_add(site.rva), site.bytes))
    {
        return 0;
    }
    crate::observers::bind_targets(
        [JITTER, PROCESS, RESOLUTION, END, UPLOAD].map(|site| base.wrapping_add(site.rva)),
    );
    1
}

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    unsafe { Probe::detect(probe, |probe| probe.matches(EXECUTABLE, SHA256)) }
}

/// Publishes the GameSDK table backed by the Rust session. A short output may contain only size.
///
/// # Safety
/// `out` is null or points to the caller's writable sized API table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_publish_api(
    requested: u32,
    out: *mut rsf_game_plugin_api,
) -> rsf_result {
    let info = rsf_game_info {
        id: c"skyrim-se".as_ptr(),
        name: c"Skyrim Special Edition".as_ptr(),
        version: VERSION.as_ptr().cast(),
        rendering_ready: 0,
        status: STATUS.as_ptr(),
    };
    let hooks = rsf_game_hooks_api {
        prepare: Some(session::prepare),
        start: Some(session::start),
        quiesce: Some(session::quiesce),
        stop: Some(session::stop),
        status: Some(session::status),
    };
    unsafe { publish_api(requested, out, info, detect, hooks) }
}
