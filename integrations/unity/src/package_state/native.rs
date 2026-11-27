// SPDX-License-Identifier: MIT
//! Package entry points backed by the linked Unity SDK and Runtime shims.
use super::{PACKAGE, routing, services};
use crate::bridge_native::package::*;
use crate::preview::*;
use crate::sys::{rsf_sharpening_core_defaults, rsf_sharpening_options};
use crate::vulkan_state::bridge::rsf_uvk_bridge_update_window;
use std::ffi::{c_int, c_void};
use std::sync::atomic::Ordering::SeqCst;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
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
        rsf_unity_package_restore as restore, rsf_unity_package_subclass as subclass,
        rsf_unity_package_timestamp as timestamp, rsf_unity_package_window_class as window_class,
    };
    pub(in crate::package_state) use super::{
        rsf_unity_package_prepare_preview as preview_prepare,
        rsf_unity_package_renderer_vulkan as renderer_vulkan,
        rsf_unity_package_texture_desc as texture_desc, sharpening_defaults,
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
        rsf_callback_host_start as host_start, rsf_callback_host_status_get as host_status,
        rsf_callback_host_stop as host_stop, rsf_d3d11_present_backend as present_backend,
        rsf_d3d11_present_generation as present_generation,
        rsf_d3d11_present_switch_result as present_switch_result, rsf_fg12_cpu as cpu,
        rsf_fg12_end_session as end_session, rsf_fg12_install as fg_install,
        rsf_fg12_status as fg_status, rsf_fg12_window as fg_window,
        rsf_native_latency_status_get as latency_status,
        rsf_render_settings_apply as settings_apply, rsf_render_settings_get as settings_get,
    };
    pub(in crate::package_state) use crate::vulkan_state::bridge::rsf_uvk_bridge_owns_window as vulkan_owns;
    pub(in crate::package_state) use rsf_windows::rsf_win32_input_kind as input_kind;
}
pub(super) fn sharpening_defaults() -> rsf_sharpening_options {
    unsafe { rsf_sharpening_core_defaults() }
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
