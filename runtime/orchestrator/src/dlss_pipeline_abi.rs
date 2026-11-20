// SPDX-License-Identifier: MIT
// Hand-written mirrors of rescaleframe/dlss_pipeline.h and dlss_pipeline_platform.h.
// bindgen-cli is not installed here; dlss_pipeline.cpp asserts the same sizes and
// offsets with static_assert.
#![allow(non_camel_case_types)]
use crate::d3d11_output_abi::rsf_d3d11_texture_desc;
use crate::native_sr_abi::rsf_motion_decode_params;
use crate::render_abi::{
    rsf_dlss_frame, rsf_motion_resolve_params, rsf_pipeline_camera_frame, rsf_sharpening_options,
};
use std::ffi::{c_char, c_void};

pub type Raw = *mut c_void;
pub type rsf_dlss_pipeline_log_fn = Option<unsafe extern "C" fn(Raw, *const c_char)>;
pub type rsf_dlssp_log_fn = Option<unsafe extern "C" fn(Raw, *const c_char)>;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct rsf_dlss_pipeline_setup {
    pub struct_size: u32,
    pub abi_version: u32,
    pub streamline_directory_utf8: *const c_char,
    pub streamline_log_directory_utf8: *const c_char,
    pub require_signature: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub quality: u32,
    pub motion: rsf_motion_decode_params,
    pub log: rsf_dlss_pipeline_log_fn,
    pub log_user: Raw,
    pub engine: u32,
    pub engine_version_utf8: *const c_char,
    pub project_id_utf8: *const c_char,
    pub fsr2_directory_utf8: *const c_char,
    pub fsr3_directory_utf8: *const c_char,
    pub fsr4_directory_utf8: *const c_char,
    pub xess_directory_utf8: *const c_char,
    pub view_space_to_meters: f32,
    pub standard_depth: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rsf_dlss_pipeline_status {
    pub struct_size: u32,
    pub running: u32,
    pub dlss_supported: u32,
    pub render_width: u32,
    pub render_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub frames_evaluated: u64,
    pub frames_refused: u64,
    pub last_result: i32,
    pub layer_frames_evaluated: u64,
    pub layer_frames_refused: u64,
    pub layer_width: u32,
    pub layer_height: u32,
    pub layer_last_result: i32,
    pub backend: u32,
    pub requested_backend: u32,
    pub last_switch_result: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct rsf_dlss_pipeline_layer {
    pub struct_size: u32,
    pub abi_version: u32,
    pub color: Raw,
    pub depth: Raw,
    pub width: u32,
    pub height: u32,
    pub camera: *const rsf_pipeline_camera_frame,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rsf_dlssp_range {
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub max_width: u32,
    pub max_height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rsf_dlssp_support {
    pub supported: u32,
    pub driver_out_of_date: u32,
    pub os_out_of_date: u32,
    pub no_supported_adapter: u32,
    pub required_major: u32,
    pub required_minor: u32,
    pub detected_major: u32,
    pub detected_minor: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rsf_dlssp_sharpen_frame {
    pub depth_inverted: u32,
    pub motion_x: f32,
    pub motion_y: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

#[repr(C)]
pub struct rsf_dlssp_platform {
    pub present_host: unsafe extern "C" fn() -> Raw,
    pub share_host: unsafe extern "C" fn(Raw, rsf_dlssp_log_fn, Raw) -> i32,
    pub load: unsafe extern "C" fn(
        *const rsf_dlss_pipeline_setup,
        *const c_char,
        *const c_char,
        rsf_dlssp_log_fn,
    ) -> i32,
    pub set_device: unsafe extern "C" fn(Raw) -> i32,
    pub query_support: unsafe extern "C" fn(*mut rsf_dlssp_support) -> i32,
    pub plan: unsafe extern "C" fn(u32, u32, u32, *mut rsf_dlssp_range) -> i32,
    pub release_resources: unsafe extern "C" fn() -> i32,
    pub release_viewport: unsafe extern "C" fn(u32) -> i32,
    pub shutdown: unsafe extern "C" fn() -> i32,
    pub evaluate_layer: unsafe extern "C" fn(Raw, *const rsf_dlss_frame) -> i32,
    pub preset_valid: unsafe extern "C" fn(u32) -> i32,
    pub fg_evaluate: unsafe extern "C" fn(Raw, *const rsf_dlss_frame) -> i32,
    pub fg_capture: unsafe extern "C" fn(Raw, *const rsf_dlss_frame),
    pub native_sr_release: unsafe extern "C" fn(),
    pub texture_desc: unsafe extern "C" fn(Raw, *mut rsf_d3d11_texture_desc),
    pub create_texture: unsafe extern "C" fn(Raw, *const rsf_d3d11_texture_desc) -> Raw,
    pub clear_black: unsafe extern "C" fn(Raw, Raw),
    pub create_zeroed: unsafe extern "C" fn(Raw, *const rsf_d3d11_texture_desc, u32) -> Raw,
    pub decode_create: unsafe extern "C" fn(Raw, u32, u32, u32, rsf_dlssp_log_fn, *mut Raw) -> i32,
    pub decode_run: unsafe extern "C" fn(Raw, Raw, Raw, *const rsf_motion_decode_params) -> i32,
    pub decode_texture: unsafe extern "C" fn(Raw) -> Raw,
    pub decode_destroy: unsafe extern "C" fn(Raw),
    pub resolve_create: unsafe extern "C" fn(Raw, u32, u32) -> Raw,
    pub resolve_run:
        unsafe extern "C" fn(Raw, Raw, Raw, Raw, *const rsf_motion_resolve_params) -> i32,
    pub resolve_motion: unsafe extern "C" fn(Raw) -> Raw,
    pub resolve_depth: unsafe extern "C" fn(Raw) -> Raw,
    pub resolve_destroy: unsafe extern "C" fn(Raw),
    pub fidelity_create: unsafe extern "C" fn(Raw) -> Raw,
    pub fidelity_run: unsafe extern "C" fn(Raw, Raw, Raw, Raw, Raw, u32, *const f32) -> i32,
    pub fidelity_destroy: unsafe extern "C" fn(Raw),
    pub transport_create: unsafe extern "C" fn(Raw) -> Raw,
    pub transport_encode: unsafe extern "C" fn(Raw, Raw, Raw, Raw, u32, u32, *mut Raw) -> i32,
    pub transport_decode:
        unsafe extern "C" fn(Raw, Raw, Raw, Raw, u32, u32, Raw, u32, u32, *const f32) -> i32,
    pub transport_destroy: unsafe extern "C" fn(Raw),
    pub sharpening_valid: unsafe extern "C" fn(*const rsf_sharpening_options) -> i32,
    pub sharpening_defaults: unsafe extern "C" fn(*mut rsf_sharpening_options),
    pub vendor_sharpness: unsafe extern "C" fn(*const rsf_sharpening_options, u32) -> f32,
    pub sharpen: unsafe extern "C" fn(
        *mut Raw,
        Raw,
        Raw,
        Raw,
        Raw,
        Raw,
        *const rsf_sharpening_options,
        *const rsf_dlssp_sharpen_frame,
    ) -> i32,
    pub sharpen_destroy: unsafe extern "C" fn(Raw),
    pub alternate_create: unsafe extern "C" fn(
        Raw,
        u32,
        u32,
        *const *const c_char,
        f32,
        u32,
        u32,
        rsf_dlssp_log_fn,
        Raw,
        *mut Raw,
    ) -> i32,
    pub input_dump: unsafe extern "C" fn(Raw, *const c_char, *const c_char, Raw, u32, f32),
    pub dump_bytes:
        unsafe extern "C" fn(Raw, Raw, Raw, *const c_char, u32, f32, rsf_dlssp_log_fn) -> i32,
    pub dump_image:
        unsafe extern "C" fn(Raw, Raw, Raw, *const c_char, u32, f32, rsf_dlssp_log_fn) -> i32,
}

const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<rsf_dlss_pipeline_setup>() == 168);
    assert!(offset_of!(rsf_dlss_pipeline_setup, require_signature) == 24);
    assert!(offset_of!(rsf_dlss_pipeline_setup, motion) == 40);
    assert!(offset_of!(rsf_dlss_pipeline_setup, log) == 88);
    assert!(offset_of!(rsf_dlss_pipeline_setup, engine) == 104);
    assert!(offset_of!(rsf_dlss_pipeline_setup, engine_version_utf8) == 112);
    assert!(offset_of!(rsf_dlss_pipeline_setup, xess_directory_utf8) == 152);
    assert!(offset_of!(rsf_dlss_pipeline_setup, view_space_to_meters) == 160);
    assert!(offset_of!(rsf_dlss_pipeline_setup, standard_depth) == 164);
    assert!(size_of::<rsf_dlss_pipeline_status>() == 96);
    assert!(offset_of!(rsf_dlss_pipeline_status, frames_evaluated) == 32);
    assert!(offset_of!(rsf_dlss_pipeline_status, last_result) == 48);
    assert!(offset_of!(rsf_dlss_pipeline_status, layer_frames_evaluated) == 56);
    assert!(offset_of!(rsf_dlss_pipeline_status, layer_width) == 72);
    assert!(offset_of!(rsf_dlss_pipeline_status, layer_last_result) == 80);
    assert!(offset_of!(rsf_dlss_pipeline_status, last_switch_result) == 92);
    assert!(size_of::<rsf_dlss_pipeline_layer>() == 40);
    assert!(offset_of!(rsf_dlss_pipeline_layer, width) == 24);
    assert!(offset_of!(rsf_dlss_pipeline_layer, camera) == 32);
    assert!(size_of::<rsf_dlssp_range>() == 24);
    assert!(size_of::<rsf_dlssp_support>() == 32);
    assert!(size_of::<rsf_dlssp_sharpen_frame>() == 20);
    assert!(size_of::<rsf_dlssp_platform>() == 43 * size_of::<usize>());
};
