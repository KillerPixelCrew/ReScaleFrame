// SPDX-License-Identifier: MIT
//! Rust views of the shared D3D11 SR table, pipeline and overlay report C ABI.
#![allow(non_camel_case_types)]

use crate::{rsf_game_log_fn, rsf_game_render_pass, rsf_result};
use core::ffi::{c_char, c_void};

pub const RSF_D3D11_SR_API_VERSION: u32 = 4;

pub type rsf_get_d3d11_sr_api_fn =
    Option<unsafe extern "C" fn(*mut rsf_d3d11_sr_api) -> rsf_result>;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct rsf_d3d11_sr_api {
    pub struct_size: u32,
    pub abi_version: u32,
    pub pipeline_abi_version: u32,
    pub start: Option<unsafe extern "C" fn(*mut c_void, *const rsf_dlss_pipeline_setup) -> i32>,
    pub evaluate: Option<unsafe extern "C" fn(*mut c_void, *const rsf_game_render_pass) -> i32>,
    pub reset: Option<unsafe extern "C" fn() -> i32>,
    pub stop: Option<unsafe extern "C" fn() -> i32>,
    pub status: Option<unsafe extern "C" fn(*mut rsf_dlss_pipeline_status) -> i32>,
    pub select_backend: Option<unsafe extern "C" fn(u32, *mut u32, *mut u32) -> i32>,
    pub set_quality: Option<unsafe extern "C" fn(u32, *mut u32, *mut u32) -> i32>,
    pub set_preset: Option<unsafe extern "C" fn(u32) -> i32>,
    pub resize_output: Option<unsafe extern "C" fn(u32, u32, *mut u32, *mut u32) -> i32>,
    pub output_texture: Option<unsafe extern "C" fn() -> *mut c_void>,
    pub request_dump: Option<unsafe extern "C" fn(*const c_char) -> i32>,
    // rsf_sharpening_options is opaque here. The adapter validates this slot but does not call it.
    pub set_sharpening: Option<unsafe extern "C" fn(*const c_void) -> i32>,
}

const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<rsf_d3d11_sr_api>() == 112);
    assert!(offset_of!(rsf_d3d11_sr_api, start) == 16);
    assert!(offset_of!(rsf_d3d11_sr_api, set_sharpening) == 104);
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
};

pub const RSF_DLSS_PIPELINE_ABI_VERSION: u32 = 5;
#[repr(C)]
#[derive(Copy, Clone)]
pub struct rsf_motion_decode_params {
    pub struct_size: u32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub bias_x: f32,
    pub bias_y: f32,
    pub output_scale_x: f32,
    pub output_scale_y: f32,
    pub invalid_value: f32,
    pub zero_means_unwritten: u32,
    pub validity_enabled: u32,
    pub validity_channel: u32,
    pub validity_threshold: f32,
}
#[allow(clippy::unnecessary_operation, clippy::identity_op)]
const _: () = {
    ["Size of rsf_motion_decode_params"]
        [::core::mem::size_of::<rsf_motion_decode_params>() - 48usize];
    ["Alignment of rsf_motion_decode_params"]
        [::core::mem::align_of::<rsf_motion_decode_params>() - 4usize];
    ["Offset of field: rsf_motion_decode_params::struct_size"]
        [::core::mem::offset_of!(rsf_motion_decode_params, struct_size) - 0usize];
    ["Offset of field: rsf_motion_decode_params::scale_x"]
        [::core::mem::offset_of!(rsf_motion_decode_params, scale_x) - 4usize];
    ["Offset of field: rsf_motion_decode_params::scale_y"]
        [::core::mem::offset_of!(rsf_motion_decode_params, scale_y) - 8usize];
    ["Offset of field: rsf_motion_decode_params::bias_x"]
        [::core::mem::offset_of!(rsf_motion_decode_params, bias_x) - 12usize];
    ["Offset of field: rsf_motion_decode_params::bias_y"]
        [::core::mem::offset_of!(rsf_motion_decode_params, bias_y) - 16usize];
    ["Offset of field: rsf_motion_decode_params::output_scale_x"]
        [::core::mem::offset_of!(rsf_motion_decode_params, output_scale_x) - 20usize];
    ["Offset of field: rsf_motion_decode_params::output_scale_y"]
        [::core::mem::offset_of!(rsf_motion_decode_params, output_scale_y) - 24usize];
    ["Offset of field: rsf_motion_decode_params::invalid_value"]
        [::core::mem::offset_of!(rsf_motion_decode_params, invalid_value) - 28usize];
    ["Offset of field: rsf_motion_decode_params::zero_means_unwritten"]
        [::core::mem::offset_of!(rsf_motion_decode_params, zero_means_unwritten) - 32usize];
    ["Offset of field: rsf_motion_decode_params::validity_enabled"]
        [::core::mem::offset_of!(rsf_motion_decode_params, validity_enabled) - 36usize];
    ["Offset of field: rsf_motion_decode_params::validity_channel"]
        [::core::mem::offset_of!(rsf_motion_decode_params, validity_channel) - 40usize];
    ["Offset of field: rsf_motion_decode_params::validity_threshold"]
        [::core::mem::offset_of!(rsf_motion_decode_params, validity_threshold) - 44usize];
};

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
    pub log: rsf_game_log_fn,
    pub log_user: *mut c_void,
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

pub const RSF_OVERLAY_SR_INPUT_COLOR: u32 = 1;
pub const RSF_OVERLAY_SR_INPUT_DEPTH: u32 = 2;
pub const RSF_OVERLAY_SR_INPUT_MOTION: u32 = 4;
pub const RSF_OVERLAY_SR_INPUT_EXPOSURE: u32 = 8;
pub const RSF_OVERLAY_SR_INPUT_JITTER: u32 = 16;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct rsf_overlay_sr_report {
    pub struct_size: u32,
    pub effective_backend: u32,
    pub effective_quality: u32,
    pub render_width: u32,
    pub render_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub frames_evaluated: u32,
    pub frames_refused: u32,
    pub last_result: i32,
    pub inputs: u32,
    pub quality_locked: u32,
    pub jitter_pixels: [f32; 2usize],
    pub reason: [::std::os::raw::c_char; 96usize],
}
#[allow(clippy::unnecessary_operation, clippy::identity_op)]
const _: () = {
    ["Size of rsf_overlay_sr_report"][::std::mem::size_of::<rsf_overlay_sr_report>() - 152usize];
    ["Alignment of rsf_overlay_sr_report"]
        [::std::mem::align_of::<rsf_overlay_sr_report>() - 4usize];
    ["Offset of field: rsf_overlay_sr_report::struct_size"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, struct_size) - 0usize];
    ["Offset of field: rsf_overlay_sr_report::effective_backend"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, effective_backend) - 4usize];
    ["Offset of field: rsf_overlay_sr_report::effective_quality"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, effective_quality) - 8usize];
    ["Offset of field: rsf_overlay_sr_report::render_width"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, render_width) - 12usize];
    ["Offset of field: rsf_overlay_sr_report::render_height"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, render_height) - 16usize];
    ["Offset of field: rsf_overlay_sr_report::output_width"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, output_width) - 20usize];
    ["Offset of field: rsf_overlay_sr_report::output_height"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, output_height) - 24usize];
    ["Offset of field: rsf_overlay_sr_report::frames_evaluated"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, frames_evaluated) - 28usize];
    ["Offset of field: rsf_overlay_sr_report::frames_refused"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, frames_refused) - 32usize];
    ["Offset of field: rsf_overlay_sr_report::last_result"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, last_result) - 36usize];
    ["Offset of field: rsf_overlay_sr_report::inputs"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, inputs) - 40usize];
    ["Offset of field: rsf_overlay_sr_report::quality_locked"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, quality_locked) - 44usize];
    ["Offset of field: rsf_overlay_sr_report::jitter_pixels"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, jitter_pixels) - 48usize];
    ["Offset of field: rsf_overlay_sr_report::reason"]
        [::std::mem::offset_of!(rsf_overlay_sr_report, reason) - 56usize];
};
impl Default for rsf_overlay_sr_report {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
