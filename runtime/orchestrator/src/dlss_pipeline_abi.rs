// SPDX-License-Identifier: MIT
// Public layer binding and the pipeline's private records.
#![allow(non_camel_case_types)]
pub use crate::sys::{rsf_dlss_plan, rsf_dlss_support};
pub use rsf_game_sys::rsf_game_log_fn as rsf_dlss_pipeline_log_fn;
pub use rsf_game_sys::{rsf_dlss_pipeline_setup, rsf_dlss_pipeline_status};
use std::ffi::{c_char, c_void};

pub type Raw = *mut c_void;
pub type rsf_dlssp_log_fn = Option<unsafe extern "C" fn(Raw, *const c_char)>;

pub use crate::sys::{RSF_DLSS_PRESET_MASK, rsf_dlss_pipeline_layer};

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rsf_dlssp_sharpen_frame {
    pub depth_inverted: u32,
    pub motion_x: f32,
    pub motion_y: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}
