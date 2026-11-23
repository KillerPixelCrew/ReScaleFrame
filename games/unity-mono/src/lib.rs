// SPDX-License-Identifier: MIT
//! Unity Mono plugin: loader startup and the GameSDK lifecycle over the shared Unity package state
//! and render bridge (`rsf-unity`, built with its `mono-plugin` subset).
#![cfg(any(windows, test))]

// The plugin gets the Runtime from CMake; unit tests link the crate.
#[cfg(all(test, windows))]
use rsf_orchestrator as _;
#[cfg(all(test, windows))]
use rsf_runtime_common as _;

use rsf_unity::{bridge_state, generation, runtime_config, sys};

#[cfg(windows)]
mod lifecycle;
#[cfg(windows)]
mod mono;
#[cfg(windows)]
mod runtime_setup;
mod startup_policy;

use rsf_game_sys::session::Probe;
use rsf_game_sys::{rsf_detection, rsf_game_probe};
use std::ffi::CStr;

/// The shared engine plugin starts with a researched-build allowlist. A Unity-looking file layout
/// alone does not establish that the managed adapter is compatible with a game.
const BUILDS: &[(&CStr, &CStr)] = &[(
    c"DragNWash.exe",
    c"5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc",
)];

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    unsafe {
        Probe::detect(probe, |probe| {
            BUILDS
                .iter()
                .any(|(executable, sha256)| probe.matches(executable, sha256))
        })
    }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_game_plugin_api(
    requested_abi: u32,
    out: *mut rsf_game_sys::rsf_game_plugin_api,
) -> rsf_game_sys::rsf_result {
    use rsf_game_sys::session::publish_api;
    use rsf_game_sys::{rsf_game_hooks_api, rsf_game_info};
    const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

    let info = rsf_game_info {
        id: c"unity-mono".as_ptr(),
        name: c"Unity Mono".as_ptr(),
        version: VERSION.as_ptr().cast(),
        rendering_ready: 0,
        status: c"Unity Mono SR adapter built; game validation is pending.".as_ptr(),
    };
    let hooks = rsf_game_hooks_api {
        prepare: Some(lifecycle::prepare),
        start: Some(lifecycle::start),
        quiesce: Some(lifecycle::quiesce),
        stop: Some(lifecycle::stop),
        status: Some(lifecycle::status),
    };
    unsafe { publish_api(requested_abi, out, info, detect, hooks) }
}

/// The Unity SDK leaves of integrations/unity/native/render_bridge.c, which CMake builds against
/// the untracked Unity headers. rsf-unity's bridge_native calls them, so the unit-test binary
/// needs the symbols; none of these tests reach the D3D12 route, and these report it absent.
#[cfg(all(test, windows))]
mod unity_sdk_absent {
    use std::ffi::{c_int, c_void};
    use std::ptr;

    #[unsafe(no_mangle)]
    extern "C" fn rsf_unity_d3d12_sdk_engine() -> *const c_void {
        ptr::null()
    }
    #[unsafe(no_mangle)]
    extern "C" fn rsf_unity_d3d12_sdk_reserve_events() -> c_int {
        0
    }
    #[unsafe(no_mangle)]
    extern "C" fn rsf_unity_d3d12_sdk_configure_event(_: c_int, _: bool, _: u32, _: bool) {}
    #[unsafe(no_mangle)]
    extern "C" fn rsf_unity_d3d12_sdk_available() -> bool {
        false
    }
    #[unsafe(no_mangle)]
    extern "C" fn rsf_unity_d3d12_sdk_lookup(_: *mut c_void) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsf_game_sys::{RSF_GAME_RECOGNIZED as RECOGNIZED, RSF_GAME_UNKNOWN as UNKNOWN};
    use std::ptr;

    const AMD64: u16 = 0x8664;

    #[test]
    fn detection_needs_the_researched_name_and_hash() {
        let hash = c"5FDFFFE386A2F43B77626CD3D70554D84C6588C94D309544924D6FAB088DDAFC";
        let mut probe = rsf_game_probe {
            struct_size: size_of::<rsf_game_probe>() as u32,
            pe_machine: AMD64,
            reserved: 0,
            executable_name_utf8: c"dragnwash.EXE".as_ptr(),
            sha256_hex: hash.as_ptr(),
        };
        assert_eq!(unsafe { detect(&probe) }, RECOGNIZED);
        probe.sha256_hex = c"unknown".as_ptr();
        assert_eq!(unsafe { detect(&probe) }, UNKNOWN);
        probe.sha256_hex = hash.as_ptr();
        probe.executable_name_utf8 = c"OtherGame.exe".as_ptr();
        assert_eq!(unsafe { detect(&probe) }, UNKNOWN);
        probe.executable_name_utf8 = ptr::null();
        assert_eq!(unsafe { detect(&probe) }, UNKNOWN);
        probe.executable_name_utf8 = c"DragNWash.exe".as_ptr();
        probe.pe_machine = 0x14c;
        assert_eq!(unsafe { detect(&probe) }, UNKNOWN);
        probe.pe_machine = AMD64;
        probe.struct_size -= 1;
        assert_eq!(unsafe { detect(&probe) }, UNKNOWN);
        assert_eq!(unsafe { detect(ptr::null()) }, UNKNOWN);
    }
}
