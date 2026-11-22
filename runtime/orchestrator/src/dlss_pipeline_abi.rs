// SPDX-License-Identifier: MIT
// Public layer binding and private Rust dispatch table.
#![allow(non_camel_case_types)]
use crate::d3d11_output_abi::rsf_d3d11_texture_desc;
pub use crate::dlss_streamline_abi::{rsf_dlss_plan, rsf_dlss_support};
use crate::gl_pipeline_abi::rsf_dlss_setup;
use crate::native_sr_abi::rsf_motion_decode_params;
use crate::render_abi::{rsf_dlss_frame, rsf_motion_resolve_params, rsf_sharpening_options};
pub use rsf_game_sys::rsf_game_log_fn as rsf_dlss_pipeline_log_fn;
pub use rsf_game_sys::sr_abi::{rsf_dlss_pipeline_setup, rsf_dlss_pipeline_status};
use std::ffi::{c_char, c_void};

pub type Raw = *mut c_void;
pub type rsf_dlssp_log_fn = Option<unsafe extern "C" fn(Raw, *const c_char)>;

#[path = "dlss_pipeline_layer_abi.rs"]
mod layer;
pub use layer::{RSF_DLSS_PRESET_MASK, rsf_dlss_pipeline_layer};

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rsf_dlssp_sharpen_frame {
    pub depth_inverted: u32,
    pub motion_x: f32,
    pub motion_y: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

pub struct rsf_dlssp_platform {
    pub present_host: unsafe extern "C" fn() -> Raw,
    pub share_host: unsafe extern "C" fn(Raw, rsf_dlssp_log_fn, Raw) -> i32,
    pub load: unsafe extern "C" fn(*const rsf_dlss_setup) -> i32,
    pub set_device: unsafe extern "C" fn(Raw) -> i32,
    pub query_support: unsafe extern "C" fn(*mut rsf_dlss_support) -> i32,
    pub plan: unsafe extern "C" fn(*mut rsf_dlss_plan) -> i32,
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
