// SPDX-License-Identifier: MIT
//! Unity Mono game plugin: build detection, the renderer lifecycle, the Mono helper bootstrap, the
//! frame state of the shared D3D12 render bridge (bridge_state.rs), and the Runtime setup behind
//! the rsf_unity_* exports (runtime_setup.rs) with its [UnitySR] settings (runtime_config.rs).
//! bridge.cpp keeps the Unity graphics interfaces, D3D12 commands and CPU input events;
//! plugin.cpp lends the bridge through rsf_unity_plugin_platform, and runtime_host.cpp keeps the
//! exports and lends the typed Runtime calls through rsf_unity_runtime_platform. The Unity package
//! links this archive for the bridge state (shared by its D3D11 and Vulkan bridge, whose D3D12
//! submission sequencing is bridge_interop.rs, with the D3D11 bridge's graphics record and command
//! slots in d3d11_state.rs and the Vulkan bridge's admission, lifecycle and presentation in
//! vulkan_state.rs), the [UnitySR] path settings and its own settings,
//! status, session state, D3D12 SR views, presentation and session services (package_state.rs,
//! package_views.rs, package_state/presentation.rs, package_state/services.rs), whose leaves stay in
//! package.cpp. Both routes install generation with the same Unity setup (generation.rs).
#![cfg(any(windows, test))]

mod bridge_abi;
mod bridge_interop;
mod bridge_state;
mod d3d11_state;
mod generation;
#[cfg(windows)]
mod lifecycle;
#[cfg(windows)]
mod mono;
mod package_abi;
mod package_state;
mod package_views;
mod plugin_abi;
mod preview;
mod runtime_abi;
mod runtime_config;
#[cfg(windows)]
mod runtime_setup;
mod vulkan_resolver;
mod vulkan_state;

use rsf_game_sys::{rsf_detection, rsf_game_probe};
use std::ffi::CStr;

const UNKNOWN: rsf_detection = 0;
const RECOGNIZED: rsf_detection = 1;
const AMD64: u16 = 0x8664;

/// The shared engine plugin starts with a researched-build allowlist. A Unity-looking file layout
/// alone does not establish that the managed adapter is compatible with a game.
const BUILDS: &[(&[u8], &[u8])] = &[(
    b"DragNWash.exe",
    b"5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc",
)];

fn recognized(executable: &[u8], sha256: &[u8]) -> bool {
    BUILDS.iter().any(|(name, hash)| {
        name.eq_ignore_ascii_case(executable) && hash.eq_ignore_ascii_case(sha256)
    })
}

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
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
    if recognized(executable.to_bytes(), sha256.to_bytes()) {
        RECOGNIZED
    } else {
        UNKNOWN
    }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_mono_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_sys::rsf_game_plugin_api,
    platform: *const plugin_abi::rsf_unity_plugin_platform,
) -> rsf_game_sys::rsf_result {
    use rsf_game_sys::{
        RSF_GAME_ABI_VERSION, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api,
    };
    const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return lifecycle::INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return lifecycle::ABI_MISMATCH;
    }
    // The bridge table is plugin.cpp's constant; an incomplete one is a build defect.
    if platform.is_null() || !lifecycle::adopt(unsafe { &*platform }) {
        return lifecycle::INVALID_ARGUMENT;
    }
    let api = rsf_game_plugin_api {
        struct_size: size_of::<rsf_game_plugin_api>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        info: rsf_game_info {
            id: c"unity-mono".as_ptr(),
            name: c"Unity Mono".as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status: c"Unity Mono SR adapter built; game validation is pending.".as_ptr(),
        },
        detect: Some(detect),
        hooks: rsf_game_hooks_api {
            prepare: Some(lifecycle::prepare),
            start: Some(lifecycle::start),
            quiesce: Some(lifecycle::quiesce),
            stop: Some(lifecycle::stop),
            status: Some(lifecycle::status),
        },
    };
    unsafe { out.write(api) };
    lifecycle::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

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
