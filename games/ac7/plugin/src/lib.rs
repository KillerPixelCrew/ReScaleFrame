// SPDX-License-Identifier: MIT
//! `ReScaleFrame.Game.AC7.dll`'s Rust: the exact recognized build, the GameSDK table and the
//! renderer lifecycle (`lifecycle.rs`), and the native renderer: the scene-precision and temporal
//! velocity patch transactions (`patches.rs`), the hook body guards (`guards.rs`), the
//! prepare/start/quiesce/stop transaction with its retirement admission (`native_renderer/transaction.rs`), the
//! TrueSky plugin patches and call relays (`truesky.rs`), the renderer object, detours, engine
//! relays and pass leases (`native_renderer.rs`), the queued RHI render scopes of
//! ac7_render_scope.h (`render_scope.rs`), the scoped primary-view facts and TrueSky's D3D11
//! kernels. The detours reach MinHook through rsf-minhook-sys. Native code keeps the SEH-guarded
//! reads (ac7_memory.c), the shader factory exception leaf and the input facts
//! (native_renderer_platform.c) and the render scopes' C create calls (render_scope_entry.c).
//! plugin.c binds that call table and keeps the exported rsf_get_game_plugin_api.
//!
//! The view reader, the code sites, the carrier policy, the motion capture and the translucent
//! velocity pass are rsf-ac7, which the carriers link without this crate and its hooks.

pub use rsf_ac7::sys;
// The adapter DLL gets the D3D11 leaves rsf-ac7 calls from their CMake archive; unit tests link
// the crate directly.
#[cfg(all(test, windows))]
use rsf_graphics as _;
mod cloud_depth;
#[cfg(windows)]
mod com_resource;
mod guards;
mod lifecycle;
#[cfg(windows)]
mod native_renderer;
mod patches;
mod render_scope;
mod scope_facts;
mod sites;
mod truesky;

use lifecycle::Lifecycle;
use rsf_game_sys::call_header::validate;
use rsf_game_sys::session::{Probe, publish_api};
use rsf_game_sys::{
    rsf_detection, rsf_game_control_args, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api,
    rsf_game_prepare_args, rsf_game_probe, rsf_game_renderer_status, rsf_game_start_args,
    rsf_result,
};
use std::ffi::CStr;

use rsf_ac7::{EXECUTABLE, EXECUTABLE_SHA256};
pub(crate) use rsf_game_sys::{
    RSF_ERROR_BUSY, RSF_ERROR_INVALID_ARGUMENT, RSF_ERROR_NATIVE_REFUSED, RSF_ERROR_NOT_READY,
    RSF_OK,
};
const ID: &CStr = c"ac7";
const NAME: &CStr = c"Ace Combat 7: Skies Unknown";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const STATUS: &CStr =
    c"Native SR graph and view producer built and synthetic-tested. Game validation is pending.";

static LIFECYCLE: Lifecycle = Lifecycle::new();

/// Exact-build identity: the AMD64 executable name and its SHA-256, both ASCII case-insensitive.
unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    unsafe { Probe::detect(probe, |probe| probe.matches(EXECUTABLE, EXECUTABLE_SHA256)) }
}

unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    let host = unsafe { (*args).host };
    let host_valid = unsafe { validate(host) };
    if host_valid != RSF_OK {
        return host_valid;
    }
    let host = unsafe { &*host };
    if host.session_id == 0 {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    LIFECYCLE.prepare(host)
}

unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    LIFECYCLE.start()
}

unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    LIFECYCLE.quiesce()
}

unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    LIFECYCLE.stop()
}

unsafe extern "C" fn status(output: *mut rsf_game_renderer_status) -> rsf_result {
    let valid = unsafe { validate(output.cast_const()) };
    if valid != RSF_OK {
        return valid;
    }
    LIFECYCLE.status(unsafe { &mut *output })
}

/// Publishes the GameSDK table for the exported rsf_get_game_plugin_api (plugin.c).
///
/// # Safety
/// `out` is null or a caller-owned plugin table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
) -> rsf_result {
    let info = rsf_game_info {
        id: ID.as_ptr(),
        name: NAME.as_ptr(),
        version: VERSION.as_ptr().cast(),
        rendering_ready: 0,
        status: STATUS.as_ptr(),
    };
    let hooks = rsf_game_hooks_api {
        prepare: Some(prepare),
        start: Some(start),
        quiesce: Some(quiesce),
        stop: Some(stop),
        status: Some(status),
    };
    unsafe { publish_api(requested_abi, out, info, detect, hooks) }
}

#[cfg(test)]
mod tests;
