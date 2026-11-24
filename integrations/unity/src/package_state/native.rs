// SPDX-License-Identifier: MIT
//! Package entry points backed by the linked Unity SDK and Runtime shims.
use super::services::backend_log;
use super::{PACKAGE, routing, services};
use crate::bridge_native::package::*;
use crate::generation::RSF_DLSS_ENGINE_UNITY;
use crate::preview::*;
use crate::sys::{
    RSF_DLSS_ABI_VERSION, RSF_SR12_PLAN_ABI_VERSION, rsf_d3d11_present_host, rsf_dlss_setup,
    rsf_fg_default_backend, rsf_sharpening_core_defaults, rsf_sharpening_core_valid,
    rsf_sharpening_options, rsf_sr12, rsf_sr12_create_planned, rsf_sr12_plan_result,
    rsf_sr12_setup, rsf_unity_package_view_setup,
};
use crate::vulkan_state::bridge::rsf_uvk_bridge_update_window;
use rsf_game_sys::backend_result;
use std::ffi::{c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::Ordering::SeqCst;
use windows::Win32::Graphics::{
    Direct3D12::{ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource},
    Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory4},
};
use windows::core::Interface;

unsafe extern "C" {
    // package.c: the Unity SDK renderer query.
    pub(super) fn rsf_unity_package_renderer_vulkan(graphics: *mut c_void) -> u32;
}

/// The leaves' production binding (package_state's `raw`): the Runtime's exports, this crate's
/// bridges and Win32 calls, and package.c's renderer query. The tests bind fakes instead.
pub(super) mod raw {
    pub(in crate::package_state) use super::super::windows::{
        rsf_unity_package_create_directory as create_directory,
        rsf_unity_package_local_app_data as local_app_data,
        rsf_unity_package_log_close as log_close, rsf_unity_package_log_open as log_open,
        rsf_unity_package_log_write as log_write, rsf_unity_package_player_window as player_window,
        rsf_unity_package_restore as restore, rsf_unity_package_session_seed as session_seed,
        rsf_unity_package_subclass as subclass, rsf_unity_package_timestamp as timestamp,
        rsf_unity_package_window_class as window_class,
    };
    pub(in crate::package_state) use super::{
        rsf_unity_package_create_view as sr_create,
        rsf_unity_package_default_fg as fg_default_backend,
        rsf_unity_package_prepare_preview as preview_prepare,
        rsf_unity_package_renderer_vulkan as renderer_vulkan,
        rsf_unity_package_texture_desc as texture_desc, sharpening_defaults, sharpening_valid,
    };
    pub(in crate::package_state) use crate::bridge_native::package::{
        rsf_unity_package_bridge_finish_retirement as bridge_finish_retirement,
        rsf_unity_package_bridge_load as bridge_load, rsf_unity_package_bridge_open as bridge_open,
        rsf_unity_package_bridge_prepare as bridge_prepare,
        rsf_unity_package_bridge_try_close as bridge_try_close,
        rsf_unity_package_bridge_unload as bridge_unload,
    };
    pub(in crate::package_state) use crate::bridge_native::rsf_unity_set_engine_spatial as set_engine_spatial;
    pub(in crate::package_state) use crate::bridge_state::rsf_unity_frames_activate as bridge_activate;
    pub(in crate::package_state) use crate::preview::{
        rsf_unity_preview_owns as preview_owns, rsf_unity_preview_parent as preview_parent,
        rsf_unity_preview_present as preview_present,
    };
    pub(in crate::package_state) use crate::sys::{
        rsf_d3d11_present_backend as present_backend,
        rsf_d3d11_present_generation as present_generation,
        rsf_d3d11_present_has_owner as present_has_owner,
        rsf_d3d11_present_request as present_request,
        rsf_d3d11_present_switch_result as present_switch_result, rsf_fg12_cpu as cpu,
        rsf_fg12_end_session as end_session, rsf_fg12_hudless as fg_hudless,
        rsf_fg12_install as fg_install, rsf_fg12_options as fg_options,
        rsf_fg12_status as fg_status, rsf_fg12_submitted as fg_submitted,
        rsf_fg12_window as fg_window, rsf_native_latency_status_get as latency_status,
        rsf_sr12_destroy as sr_destroy, rsf_sr12_evaluate as sr_evaluate,
        rsf_sr12_quality_supported as sr_quality_supported,
        rsf_sr12_set_sharpening as sr_set_sharpening,
    };
    pub(in crate::package_state) use crate::vulkan_state::bridge::rsf_uvk_bridge_owns_window as vulkan_owns;
    pub(in crate::package_state) use rsf_windows::rsf_win32_input_kind as input_kind;
}
pub(super) fn sharpening_valid(options: &rsf_sharpening_options) -> bool {
    options.struct_size as usize >= size_of::<rsf_sharpening_options>()
        && unsafe { rsf_sharpening_core_valid(*options) } != 0
}

pub(super) fn sharpening_defaults() -> rsf_sharpening_options {
    unsafe { rsf_sharpening_core_defaults() }
}

/// Creates the view's SR context on the recording list's device: the creation result and the
/// context, or None when the list has no device. Runs inside the bridge's admitted SR callback
/// and borrows the recording list.
pub(super) unsafe fn rsf_unity_package_create_view(
    commands: *mut c_void,
    view: &mut rsf_unity_package_view_setup,
) -> Option<(i32, *mut rsf_sr12)> {
    let commands = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&commands) }?;
    let mut device = None::<ID3D12Device>;
    unsafe { commands.GetDevice(&mut device) }.ok()?;
    let device = device?;
    let setup = rsf_sr12_setup {
        struct_size: size_of::<rsf_sr12_setup>() as u32,
        abi_version: 1,
        device: device.as_raw(),
        backend: view.backend,
        quality: view.quality,
        output_width: view.output_width,
        output_height: view.output_height,
        render_width: view.render_width,
        render_height: view.render_height,
        inverted_depth: view.inverted_depth,
        streamline_host: unsafe { rsf_d3d11_present_host() }.cast(),
        fsr2_directory_utf8: view.fsr_utf8,
        fsr3_directory_utf8: view.fsr_utf8,
        fsr4_directory_utf8: view.fsr_utf8,
        xess_directory_utf8: view.xess_utf8,
        log: Some(backend_log),
        dlss: rsf_dlss_setup {
            struct_size: size_of::<rsf_dlss_setup>() as u32,
            abi_version: RSF_DLSS_ABI_VERSION,
            interposer_path_utf8: view.interposer_utf8,
            plugin_directory_utf8: view.streamline_utf8,
            log_directory_utf8: view.root_utf8,
            engine: RSF_DLSS_ENGINE_UNITY,
            engine_version_utf8: view.engine_version_utf8,
            project_id_utf8: view.project_id_utf8,
            require_signature: 1,
            log: Some(backend_log),
            preset: view.dlss_preset,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut plan = rsf_sr12_plan_result {
        struct_size: size_of::<rsf_sr12_plan_result>() as u32,
        abi_version: RSF_SR12_PLAN_ABI_VERSION,
        ..Default::default()
    };
    let mut output = null_mut();
    let created = unsafe { rsf_sr12_create_planned(&setup, &mut plan, &mut output) };
    if created == backend_result::OK {
        view.backend = plan.backend;
        view.render_width = plan.render_width;
        view.render_height = plan.render_height;
    }
    let result = if plan.requested_result != backend_result::OK {
        plan.requested_result
    } else {
        created
    };
    Some((result, output))
}

/// Auto's generation provider for Unity's adapter; None when the device or adapter cannot be read.
pub(super) fn rsf_unity_package_default_fg() -> Option<u32> {
    let (device, _queue) = graphics()?;
    let desc = unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|adapter| adapter.GetDesc1())
    }
    .ok()?;
    let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
    Some(unsafe { rsf_fg_default_backend(desc.VendorId, u32::from(software)) })
}

/// ID3D12Resource::GetDesc's width, height and format; zeros for a null resource.
pub(super) unsafe fn rsf_unity_package_texture_desc(resource: *mut c_void) -> (u64, u32, u32) {
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return (0, 0, 0);
    };
    let desc = unsafe { resource.GetDesc() };
    (desc.Width, desc.Height, desc.Format.0 as u32)
}

pub(super) fn rsf_unity_package_prepare_preview(width: u32, height: u32) -> bool {
    let Some((device, queue)) = graphics() else {
        return false;
    };
    unsafe {
        rsf_unity_preview_prepare(
            device.as_raw(),
            queue.as_raw(),
            width,
            height,
            Some(routing::observe_preview),
        )
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsfu_preview_region(
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    visible: u32,
) -> c_int {
    let configured = rsf_unity_preview_region(left, top, width, height, visible != 0);
    services::rsf_unity_package_preview_input();
    c_int::from(configured)
}

#[unsafe(no_mangle)]
extern "C" fn rsfu_shutdown() -> c_int {
    if PACKAGE.services.editor.load(SeqCst) {
        hide();
    }
    c_int::from(routing::shutdown())
}

/// The managed adapter's resize of the player's presentation surface.
#[unsafe(no_mangle)]
extern "C" fn rsfu_surface(width: u32, height: u32) -> c_int {
    c_int::from(rsf_uvk_bridge_update_window(width, height))
}
