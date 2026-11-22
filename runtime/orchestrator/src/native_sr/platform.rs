// SPDX-License-Identifier: MIT
use super::*;
use crate::motion_decode::{rsf_motion_decode_create, rsf_motion_decode_destroy};
use crate::motion_resolve::{rsf_motion_resolve_create, rsf_motion_resolve_destroy};
use crate::native_regions::{
    rsf_native_regions_depth_valid, rsf_native_regions_prepare, rsf_native_regions_temporal_mask,
};
use crate::native_reinsert::rsf_native_reinsert;
use crate::sys::{RSF_MOTION_DECODE_ABI_VERSION, rsf_motion_decode_setup};
use rsf_game_sys::depth_domains_requested;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32_FLOAT;

// The pipeline and FG capture owners are Rust exports of this crate. Vendor C++
// below them contains its own exceptions at the SDK leaf, so these calls need no
// native guard.
unsafe extern "C" {
    fn rsf_dlss_pipeline_resize_output(
        width: u32,
        height: u32,
        render_width: *mut u32,
        render_height: *mut u32,
    ) -> i32;
    fn rsf_dlss_pipeline_on_frame(context: Texture, frame: *const Frame) -> i32;
    fn rsf_native_fg_capture(context: Texture, frame: *const DlssFrame);
    fn rsf_dlss_pipeline_output_texture() -> Texture;
}

pub(super) unsafe fn resize_output(width: u32, height: u32) -> i32 {
    unsafe {
        rsf_dlss_pipeline_resize_output(width, height, std::ptr::null_mut(), std::ptr::null_mut())
    }
}

pub(super) unsafe fn run_frame(context: Texture, frame: &Frame) -> i32 {
    unsafe { rsf_dlss_pipeline_on_frame(context, frame) }
}

pub(super) unsafe fn capture_fg(context: Texture, frame: &DlssFrame) {
    unsafe { rsf_native_fg_capture(context, frame) }
}

/// Returns None when the depth layout is refused, otherwise whether depth domains are requested.
pub(super) unsafe fn depth_layout(pass: *const Pass) -> Option<bool> {
    let pass = unsafe { &*pass };
    let desc = unsafe { texture_desc(pass.depth) };
    unsafe { rsf_native_regions_depth_valid(&pass.depth_domains, desc.format, &pass.render_rect) }
        .then(|| depth_domains_requested(&pass.depth_domains))
}

pub(super) unsafe fn prepare(context: Texture, pass: &Pass, out: &mut [Texture; 3]) -> bool {
    let [color, depth, motion] = out;
    unsafe {
        rsf_native_regions_prepare(
            context,
            pass.color_input,
            pass.depth,
            pass.motion,
            &pass.render_rect,
            color,
            depth,
            motion,
            &pass.depth_domains,
            pass.camera.depth_inverted != 0,
        )
    }
}

pub(super) unsafe fn merge_mask(context: Texture, pass: *const Pass, frame: *mut Frame) -> bool {
    let pass = unsafe { &*pass };
    let frame = unsafe { &mut *frame };
    let (mut reactive, mut bias) = (std::ptr::null_mut(), std::ptr::null_mut());
    if !unsafe {
        rsf_native_regions_temporal_mask(
            context,
            pass.temporal_mask,
            &pass.render_rect,
            &pass.temporal_mask_weights,
            frame.reactive_mask,
            frame.bias_mask,
            &mut reactive,
            &mut bias,
        )
    } {
        return false;
    }
    frame.reactive_mask = reactive;
    frame.bias_mask = bias;
    true
}

pub(super) unsafe fn publish_output(context: Texture, pass: *const Pass) -> bool {
    let pass = unsafe { &*pass };
    unsafe {
        rsf_native_reinsert(
            context,
            rsf_dlss_pipeline_output_texture(),
            pass.color_output,
            pass.color_output_readable,
            &pass.output_rect,
        )
    }
}

pub(super) unsafe fn create_decode(device: Texture, width: u32, height: u32) -> Texture {
    let setup = rsf_motion_decode_setup {
        struct_size: size_of::<rsf_motion_decode_setup>() as u32,
        abi_version: RSF_MOTION_DECODE_ABI_VERSION,
        width,
        height,
        output_format: DXGI_FORMAT_R32G32_FLOAT.0 as u32,
        log: None,
        log_user: std::ptr::null_mut(),
    };
    let mut decode = std::ptr::null_mut();
    if unsafe { rsf_motion_decode_create(device, &setup, &mut decode) } == 0 {
        decode.cast()
    } else {
        std::ptr::null_mut()
    }
}

pub(super) unsafe fn create_resolve(device: Texture, width: u32, height: u32) -> Texture {
    let mut resolve = std::ptr::null_mut();
    if unsafe { rsf_motion_resolve_create(device, width, height, &mut resolve) } != 0 {
        resolve.cast()
    } else {
        std::ptr::null_mut()
    }
}
pub(super) unsafe extern "C" fn destroy_decode(pass: Texture) {
    unsafe { rsf_motion_decode_destroy(pass.cast()) };
}
pub(super) unsafe extern "C" fn destroy_resolve(pass: Texture) {
    unsafe { rsf_motion_resolve_destroy(pass.cast()) };
}
