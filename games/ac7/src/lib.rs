// SPDX-License-Identifier: MIT
//! Ace Combat 7 (Steam build 9855922) adapter policy linked into `ReScaleFrame.Game.AC7.dll`:
//! the exact recognized build, the GameSDK table and the renderer lifecycle (`lifecycle.rs`),
//! and for the native renderer the exact-build code sites and their admission (`sites.rs`), the
//! scene-precision and temporal velocity patch transactions (`patches.rs`), the hook body
//! guards (`guards.rs`), the prepare/start/quiesce/stop transaction with its retirement
//! admission (`renderer.rs`) and the TrueSky plugin patches and call relays with their admission,
//! rollback and retirement (`truesky.rs`), all reached through ac7_sites.h. The queued RHI render
//! scopes of ac7_render_scope.h (`render_scope.rs`) export that header's calls except the two
//! create calls, which render_scope.cpp keeps to lend its exception leaves (ac7_scope_owner.h).
//! The translucent velocity pass keeps shader compilation, DXBC transforms, D3D11 shader
//! objects and variant publication in Rust. The Rust renderer owns the replay and scope lease,
//! the history and stream modules own their decisions, and the commit module owns admission
//! and reports. velocity_pass.cpp lends the D3D11 resource allocation, draw and merge leaves.
//!
//! The rest of the native renderer stays C++ (native_renderer.cpp and the render-scope library):
//! the five exported renderer calls as wrappers, the renderer object and its fields, MinHook
//! leaves, module pinning, detours, engine relays, the TrueSky relay handlers, relay memory,
//! pass leases, SEH reads and protected code writes. Rust owns TrueSky's D3D11 kernels and
//! composite depth dispatch, scene colour references and the scoped primary-view facts. plugin.cpp
//! lends its entry points through the
//! exception-guarded leaf table in ac7_platform.h and keeps the exported
//! rsf_get_game_plugin_api. The combined carriers call `carrier.rs` for their executable and
//! window identifiers, GameSDK probe and decrypted patch policy through ac7_carrier.h.
mod carrier;
mod cloud_depth;
mod cloud_depth_abi;
#[cfg(windows)]
mod com_resource;
mod contact_shadow;
#[path = "../../../runtime/graphics/src/d3d11_observer_layout_abi.rs"]
mod d3d11_observer_layout_abi;
#[path = "../../../runtime/graphics/src/dxbc_chunks.rs"]
mod dxbc_chunks;
mod guards;
mod hook_batch;
mod lifecycle;
#[cfg(windows)]
mod motion_capture_graphics_abi;
mod motion_capture_observations;
mod motion_capture_records;
mod motion_capture_session;
mod patches;
mod platform_abi;
mod render_scope;
mod renderer;
mod scene_color;
mod scene_color_abi;
mod scope_abi;
mod scope_facts;
#[path = "../../../runtime/graphics/src/shader_hash.rs"]
mod shader_hash;
mod sites;
mod sites_abi;
mod truesky;
mod ui;
mod ui_abi;
mod velocity_commit;
mod velocity_container;
#[cfg(windows)]
mod velocity_device;
mod velocity_device_abi;
#[cfg(windows)]
mod velocity_geometry;
mod velocity_history;
mod velocity_pass_cache;
#[cfg(windows)]
mod velocity_pixel;
mod velocity_renderer;
#[cfg(windows)]
#[path = "../../../tests/ac7_velocity_shader_fixture.rs"]
mod velocity_shader_fixture;
#[cfg(any(windows, test))]
mod velocity_stage;
mod velocity_stream;
#[cfg(windows)]
mod velocity_tessellation;
#[cfg(any(windows, test))]
mod velocity_tokens;
#[cfg(windows)]
mod velocity_vertex;
mod view;
mod view_abi;

#[cfg(test)]
mod fake_code;

use lifecycle::{Leaves, Lifecycle, Renderer};
use platform_abi::{rsf_ac7_native_renderer_options, rsf_ac7_platform};
use rsf_game_sys::call_header::validate;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_control_args, rsf_game_hooks_api, rsf_game_info,
    rsf_game_plugin_api, rsf_game_prepare_args, rsf_game_probe, rsf_game_renderer_status,
    rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_char};
use std::ptr::null_mut;
use std::sync::OnceLock;

pub(crate) const RSF_OK: rsf_result = 0;
pub(crate) const RSF_ERROR_INVALID_ARGUMENT: rsf_result = -1;
pub(crate) const RSF_ERROR_ABI_MISMATCH: rsf_result = -2;
pub(crate) const RSF_ERROR_NOT_READY: rsf_result = -3;
pub(crate) const RSF_ERROR_BUSY: rsf_result = -4;
pub(crate) const RSF_ERROR_NATIVE_REFUSED: rsf_result = -5;
const UNKNOWN: rsf_detection = 0;
const RECOGNIZED: rsf_detection = 1;
const AMD64: u16 = 0x8664;

// The only recognized build. Every RVA and expected byte in sites.rs, patches.rs and
// native_renderer.cpp belongs to it.
const EXECUTABLE: &CStr = c"Ace7Game.exe";
const EXECUTABLE_SHA256: &CStr =
    c"c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f";
const ID: &CStr = c"ac7";
const NAME: &CStr = c"Ace Combat 7: Skies Unknown";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const STATUS: &CStr =
    c"Native SR graph and view producer built and synthetic-tested. Game validation is pending.";

/// A native leaf threw; the lifecycle entry reports RSF_ERROR_NOT_READY as the C++ catch did.
pub(crate) struct Threw;

/// The validated native leaf table. Every entry is present.
type PrepareFn =
    unsafe extern "C" fn(*const rsf_ac7_native_renderer_options, *mut Renderer, *mut bool) -> bool;
type QueryFn = unsafe extern "C" fn(Renderer, *mut bool) -> bool;

struct Ops {
    prepare: PrepareFn,
    start: QueryFn,
    is_active: QueryFn,
    quiesce: unsafe extern "C" fn(Renderer) -> bool,
    stop: QueryFn,
}

static OPS: OnceLock<Ops> = OnceLock::new();
static LIFECYCLE: Lifecycle = Lifecycle::new();

fn completed(returned: bool) -> Result<(), Threw> {
    if returned { Ok(()) } else { Err(Threw) }
}

impl Ops {
    fn new(table: &rsf_ac7_platform) -> Option<Self> {
        Some(Self {
            prepare: table.prepare?,
            start: table.start?,
            is_active: table.is_active?,
            quiesce: table.quiesce?,
            stop: table.stop?,
        })
    }
}

impl Leaves for Ops {
    fn prepare(
        &self,
        options: &rsf_ac7_native_renderer_options,
    ) -> Result<(Renderer, bool), Threw> {
        let leaf = self.prepare;
        let mut created = null_mut();
        let mut prepared = false;
        completed(unsafe { leaf(options, &mut created, &mut prepared) })?;
        Ok((created, prepared))
    }

    fn start(&self, renderer: Renderer) -> Result<bool, Threw> {
        let leaf = self.start;
        let mut started = false;
        completed(unsafe { leaf(renderer, &mut started) })?;
        Ok(started)
    }

    fn is_active(&self, renderer: Renderer) -> Result<bool, Threw> {
        let leaf = self.is_active;
        let mut active = false;
        completed(unsafe { leaf(renderer, &mut active) })?;
        Ok(active)
    }

    fn quiesce(&self, renderer: Renderer) -> Result<(), Threw> {
        let leaf = self.quiesce;
        completed(unsafe { leaf(renderer) })
    }

    fn stop(&self, renderer: Renderer) -> Result<bool, Threw> {
        let leaf = self.stop;
        let mut stopped = false;
        completed(unsafe { leaf(renderer, &mut stopped) })?;
        Ok(stopped)
    }
}

/// Exact-build identity: the AMD64 executable name and its SHA-256, both ASCII case-insensitive.
fn recognized(machine: u16, executable: Option<&CStr>, digest: Option<&CStr>) -> bool {
    machine == AMD64
        && executable
            .is_some_and(|name| name.to_bytes().eq_ignore_ascii_case(EXECUTABLE.to_bytes()))
        && digest.is_some_and(|hash| {
            hash.to_bytes()
                .eq_ignore_ascii_case(EXECUTABLE_SHA256.to_bytes())
        })
}

/// A borrowed probe string; null is absent.
///
/// # Safety
/// A nonnull `value` is terminated and outlives the probe call.
unsafe fn text<'a>(value: *const c_char) -> Option<&'a CStr> {
    (!value.is_null()).then(|| unsafe { CStr::from_ptr(value) })
}

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    if probe.is_null() || unsafe { probe.cast::<u32>().read() } < size_of::<rsf_game_probe>() as u32
    {
        return UNKNOWN;
    }
    let probe = unsafe { &*probe };
    let (executable, digest) =
        unsafe { (text(probe.executable_name_utf8), text(probe.sha256_hex)) };
    if recognized(probe.pe_machine, executable, digest) {
        RECOGNIZED
    } else {
        UNKNOWN
    }
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
    match OPS.get() {
        Some(ops) => LIFECYCLE.prepare(ops, host),
        None => RSF_ERROR_NOT_READY,
    }
}

unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    match OPS.get() {
        Some(ops) => LIFECYCLE.start(ops),
        None => RSF_ERROR_NOT_READY,
    }
}

unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    match OPS.get() {
        Some(ops) => LIFECYCLE.quiesce(ops),
        None => RSF_ERROR_NOT_READY,
    }
}

unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    match OPS.get() {
        Some(ops) => LIFECYCLE.stop(ops),
        None => RSF_ERROR_NOT_READY,
    }
}

unsafe extern "C" fn status(output: *mut rsf_game_renderer_status) -> rsf_result {
    let valid = unsafe { validate(output.cast_const()) };
    if valid != RSF_OK {
        return valid;
    }
    match OPS.get() {
        Some(ops) => LIFECYCLE.status(ops, unsafe { &mut *output }),
        None => RSF_ERROR_NOT_READY,
    }
}

/// Publishes the GameSDK table for the exported rsf_get_game_plugin_api.
///
/// # Safety
/// `out` is null or a caller-owned plugin table. `platform` is null or a complete table that
/// lives for the process.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
    platform: *const rsf_ac7_platform,
) -> rsf_result {
    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return RSF_ERROR_ABI_MISMATCH;
    }
    let Some(table) = (unsafe { platform.as_ref() }) else {
        return RSF_ERROR_NOT_READY;
    };
    let Some(ops) = Ops::new(table) else {
        return RSF_ERROR_NOT_READY;
    };
    let _ = OPS.get_or_init(|| ops);
    let api = rsf_game_plugin_api {
        struct_size: size_of::<rsf_game_plugin_api>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        info: rsf_game_info {
            id: ID.as_ptr(),
            name: NAME.as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status: STATUS.as_ptr(),
        },
        detect: Some(detect),
        hooks: rsf_game_hooks_api {
            prepare: Some(prepare),
            start: Some(start),
            quiesce: Some(quiesce),
            stop: Some(stop),
            status: Some(status),
        },
    };
    unsafe { out.write(api) };
    RSF_OK
}

#[cfg(test)]
mod tests;
