// SPDX-License-Identifier: MIT
use crate::vulkan_fg_abi::{
    rsf_dlssgvk_status as Status, rsf_fgvk_frame as Frame, rsf_fgvk_setup as Setup,
};
use crate::vulkan_fg_resources_abi::{rsf_vkfg_image as Image, rsf_vkfg_platform as Gpu};
use crate::vulkan_latency_abi::{rsf_slvk_call, rsf_slvk_callback};
use crate::vulkan_sr_abi::rsf_srvk_image as SourceImage;
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::c_void;

pub(super) type Handle = *mut c_void;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub(super) struct Sample {
    pub semaphore: u64,
    pub value: u64,
    pub actual_presents: u32,
    pub max_generated: u32,
    pub status: u32,
    pub min_dimension: u32,
    pub vsync: u32,
    pub dynamic_mfg: u32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct SdkCall {
    pub native: Handle,
    pub result: i32,
    pub count: u32,
    pub sample: Sample,
    pub commands: Handle,
    pub input: *const Frame,
    pub record: *const Record,
    pub images: *const Image,
}

#[repr(C)]
pub(super) struct Platform {
    pub gpu: *const Gpu,
    pub open: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    pub close: unsafe extern "C" fn(Handle),
    pub create_chain: unsafe extern "C" fn(Handle, *mut u64, *mut Handle) -> i32,
    pub images: unsafe extern "C" fn(Handle, *mut u32, *mut u64) -> i32,
    pub acquire: unsafe extern "C" fn(Handle, u64, u64, u64, *mut u32) -> i32,
    pub valid_present: unsafe extern "C" fn(Handle, *const c_void) -> u32,
    pub present: unsafe extern "C" fn(Handle, *const c_void) -> i32,
    pub invoke: unsafe extern "C" fn(*const rsf_slvk_call) -> i32,
    pub initialize: rsf_slvk_callback,
    pub configure: rsf_slvk_callback,
    pub sample: rsf_slvk_callback,
    pub constants: rsf_slvk_callback,
    pub tag: rsf_slvk_callback,
    pub release: rsf_slvk_callback,
    pub stage: unsafe extern "C" fn(Handle, Handle, *const SourceImage, *const Image),
    pub capture: unsafe extern "C" fn(Handle, Handle, *const SourceImage, *const Record, u32, u64),
    pub remember: unsafe extern "C" fn(Handle, *const Record, *const Status, u32),
    pub capture_group: unsafe extern "C" fn(Handle, u64, u32),
    pub flush: unsafe extern "C" fn(Handle),
    pub detach_capture: unsafe extern "C" fn(Handle) -> i32,
    pub timeout_epoch: unsafe extern "C" fn() -> u64,
    pub idle: unsafe extern "C" fn(Handle) -> i32,
    pub wait_inputs: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    pub take_error: unsafe extern "C" fn() -> i32,
}

const _: () = {
    assert!(size_of::<Sample>() == 40);
    assert!(size_of::<SdkCall>() == 88);
    assert!(size_of::<Platform>() == 200);
};
