// SPDX-License-Identifier: MIT
//! Hand-mirrored from `sharpening_vulkan_core.h`. The C++ header asserts the
//! same sizes and offsets.
#![allow(non_camel_case_types)]

use crate::render_abi::rsf_sharpening_options;
use crate::vulkan_fg_resources_abi::rsf_vkfg_platform;
use crate::vulkan_sr_abi::{rsf_srvk_frame, rsf_srvk_setup};
use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct rsf_sharpenvk_uniform {
    pub pool: u64,
    pub set: u64,
    pub buffer: u64,
    pub memory: u64,
    pub mapped: *mut c_void,
}

impl Default for rsf_sharpenvk_uniform {
    fn default() -> Self {
        Self {
            pool: 0,
            set: 0,
            buffer: 0,
            memory: 0,
            mapped: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct rsf_sharpenvk_layouts {
    pub descriptor_set: u64,
    pub pipeline: u64,
}

#[repr(C)]
pub struct rsf_sharpenvk_facts {
    pub width: u32,
    pub height: u32,
    pub motion_width: u32,
    pub motion_height: u32,
    pub depth_width: u32,
    pub depth_height: u32,
    pub depth_inverted: u32,
    pub exposure_texture: u32,
    pub effective_mode: u32,
    pub near_plane: f32,
    pub far_plane: f32,
    pub exposure: f32,
    pub motion_x: f32,
    pub motion_y: f32,
}

#[repr(C)]
pub struct rsf_sharpenvk_pass {
    pub set: u64,
    pub buffer: u64,
    pub pipeline: u64,
    pub pipeline_layout: u64,
    pub sampled: [u64; 4],
    pub scratch: u64,
    pub scratch_view: u64,
    pub output: u64,
    pub width: u32,
    pub height: u32,
    pub groups_x: u32,
    pub groups_y: u32,
    pub scratch_initialized: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct rsf_sharpenvk_platform {
    pub images: &'static rsf_vkfg_platform,
    pub open: unsafe extern "C" fn(*const rsf_srvk_setup, *mut *mut c_void) -> i32,
    pub close: unsafe extern "C" fn(*mut c_void),
    pub image_user: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub create_layouts: unsafe extern "C" fn(*mut c_void, *mut rsf_sharpenvk_layouts) -> i32,
    pub release_layouts: unsafe extern "C" fn(*mut c_void, *mut rsf_sharpenvk_layouts),
    pub create_pipeline: unsafe extern "C" fn(*mut c_void, u64, u32, *mut u64) -> i32,
    pub destroy_pipeline: unsafe extern "C" fn(*mut c_void, u64),
    pub create_uniform: unsafe extern "C" fn(*mut c_void, u64, *mut rsf_sharpenvk_uniform) -> i32,
    pub release_uniform: unsafe extern "C" fn(*mut c_void, *mut rsf_sharpenvk_uniform),
    pub create_view: unsafe extern "C" fn(*mut c_void, u64, u32, *mut u64) -> i32,
    pub options:
        unsafe extern "C" fn(*const rsf_sharpening_options, *mut rsf_sharpening_options) -> u32,
    pub write_constants: unsafe extern "C" fn(
        *mut c_void,
        *const rsf_sharpening_options,
        *const rsf_sharpenvk_facts,
    ),
    pub record: unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_sharpenvk_pass),
    pub log: unsafe extern "C" fn(*const rsf_srvk_frame, *const c_char),
}

const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<rsf_sharpenvk_uniform>() == 40);
    assert!(offset_of!(rsf_sharpenvk_uniform, mapped) == 32);
    assert!(size_of::<rsf_sharpenvk_layouts>() == 16);
    assert!(size_of::<rsf_sharpenvk_facts>() == 56);
    assert!(offset_of!(rsf_sharpenvk_facts, near_plane) == 36);
    assert!(size_of::<rsf_sharpenvk_pass>() == 112);
    assert!(offset_of!(rsf_sharpenvk_pass, width) == 88);
    assert!(size_of::<rsf_sharpenvk_platform>() == 15 * size_of::<usize>());
};
