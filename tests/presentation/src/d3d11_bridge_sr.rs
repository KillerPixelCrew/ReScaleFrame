// SPDX-License-Identifier: MIT
//! Synthetic SR evaluation on the presentation fixture's existing Streamline host.
use crate::d3d11_device_checks::red_half;
use crate::sys::*;
use crate::sys::{rsf_streamline_host_abort, rsf_streamline_host_begin};
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D};
use windows::core::Interface;

/// Evaluates the shared D3D12 SR once through `host` on the nine fixture `textures` (four SR inputs
/// and outputs, then five hints) and returns the frame it evaluated.
pub(super) unsafe fn shared_sr(
    host: *mut c_void,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    textures: &[ID3D11Texture2D],
    log: rsf_dlss_log_fn,
) -> Option<rsf_dlss_frame> {
    if unsafe { rsf_dlss_share_host(host, log, null_mut()) } != 0 {
        return None;
    }
    let mut support = rsf_dlss_support {
        struct_size: size_of::<rsf_dlss_support>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_dlss_query_support(&mut support) } != 0 {
        return None;
    }
    let raw = |index: usize| textures[index].as_raw();
    let mut frame = rsf_dlss_frame {
        struct_size: size_of::<rsf_dlss_frame>() as u32,
        abi_version: RSF_DLSS_ABI_VERSION,
        color_in: raw(0),
        depth: raw(1),
        motion: raw(2),
        color_out: raw(3),
        render_width: 512,
        render_height: 512,
        output_width: 512,
        output_height: 512,
        // RSF_DLSS_QUALITY_NATIVE.
        quality: 0,
        near_plane: 0.1,
        vertical_fov: 1.0,
        aspect_ratio: 1.0,
        motion_scale_x: 1.0 / 512.0,
        motion_scale_y: 1.0 / 512.0,
        camera_motion_included: 1,
        depth_inverted: 1,
        reset: 1,
        camera_forward: [0.0, 0.0, 1.0],
        camera_up: [0.0, 1.0, 0.0],
        camera_right: [1.0, 0.0, 0.0],
        // AC7's translucency hints and display-range transport.
        color_before_transparency: raw(4),
        transparency_layer: raw(5),
        reactive_mask: raw(6),
        transparency_hint: raw(7),
        bias_current_color: raw(8),
        color_encoded: 1,
        ..Default::default()
    };
    frame.camera_view_to_clip[0] = 2.0;
    frame.camera_view_to_clip[5] = 2.0;
    frame.camera_view_to_clip[11] = 1.0;
    frame.camera_view_to_clip[14] = 0.1;
    frame.clip_to_camera_view[0] = 0.5;
    frame.clip_to_camera_view[5] = 0.5;
    frame.clip_to_camera_view[11] = 10.0;
    frame.clip_to_camera_view[14] = 1.0;
    for index in [0, 5, 10, 15] {
        frame.clip_to_prev_clip[index] = 1.0;
        frame.prev_clip_to_clip[index] = 1.0;
    }
    // The pass is a C record of scalar values and nullable resource pointers.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.source_frame_id = 1;
    if unsafe { rsf_streamline_host_begin(host.cast(), 1) } != 0 {
        return None;
    }
    unsafe { rsf_native_fg_scene(&pass) };
    let evaluated = unsafe { rsf_native_fg_evaluate(context.as_raw(), &frame) };
    unsafe { rsf_native_fg_scene(null()) };
    let sample = if evaluated == 0 {
        red_half(device, context, &textures[3])
    } else {
        0
    };
    unsafe { rsf_streamline_host_abort(host.cast(), 1) };
    if unsafe { rsf_dlss_release_resources() } != 0
        || unsafe { rsf_dlss_shutdown() } != 0
        || sample == 0
        || sample >= 0x7c00
    {
        return None;
    }
    println!("shared D3D12 SR output red half={sample:04x}, one existing CPU token");
    Some(frame)
}
