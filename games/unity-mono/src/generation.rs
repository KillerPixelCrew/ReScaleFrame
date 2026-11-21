// SPDX-License-Identifier: MIT
//! The rsf_fg12_setup both Unity routes install: the game plugin's generation start
//! (runtime_setup.rs) and the Unity package's presentation (package_state/presentation.rs). They
//! pass their own provider, options, directories, window guard and log; the depth convention,
//! the provider limit, runtime switching, the HUD-less backbuffer UI mode and Unity's Streamline
//! engine type are the same for both.
use crate::runtime_abi::{RSF_UI_MODE_BACKBUFFER_HUDLESS, rsf_fg12_setup, rsf_native_fg_options};
use std::ffi::{c_char, c_int, c_void};
use std::ptr::null_mut;

/// RSF_DLSS_ENGINE_UNITY, a cast macro bindgen does not evaluate; runtime_host.cpp and package.cpp
/// assert it.
pub(crate) const RSF_DLSS_ENGINE_UNITY: u32 = 2;

/// rsf_fg12_setup's window guard and log.
pub(crate) type AcceptWindow = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
pub(crate) type Log = unsafe extern "C" fn(*mut c_void, *const c_char);

/// An install's SDK directories and Streamline identity, UTF-8 and borrowed for the install.
pub(crate) struct Sources {
    pub(crate) runtime: *const c_char,
    pub(crate) fsr3: *const c_char,
    pub(crate) fsr4: *const c_char,
    pub(crate) xess: *const c_char,
    pub(crate) streamline: *const c_char,
    pub(crate) engine_version: *const c_char,
    pub(crate) project_id: *const c_char,
}

/// Unity's depth is inverted and finite, in metres; the provider's own generated-frame limit
/// applies and it may be switched at run time; the HUD-less image comes from the backbuffer.
pub(crate) fn setup(
    backend: u32,
    options: rsf_native_fg_options,
    sources: &Sources,
    accept_window: AcceptWindow,
    log: Log,
) -> rsf_fg12_setup {
    rsf_fg12_setup {
        struct_size: size_of::<rsf_fg12_setup>() as u32,
        backend,
        max_generated_frames: u32::MAX,
        runtime_directory_utf8: sources.runtime,
        options,
        depth_inverted: 1,
        depth_infinite: 0,
        units_to_meters: 1.0,
        log: Some(log),
        user: null_mut(),
        accept_window: Some(accept_window),
        runtime_switching: 1,
        ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
        fsr3_directory_utf8: sources.fsr3,
        fsr4_directory_utf8: sources.fsr4,
        xess_directory_utf8: sources.xess,
        streamline_directory_utf8: sources.streamline,
        engine_type: RSF_DLSS_ENGINE_UNITY,
        engine_version_utf8: sources.engine_version,
        project_id_utf8: sources.project_id,
    }
}
