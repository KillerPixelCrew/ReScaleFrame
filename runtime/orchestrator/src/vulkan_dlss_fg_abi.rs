// SPDX-License-Identifier: MIT
use crate::sys::rsf_srvk_image as SourceImage;
use crate::sys::{rsf_dlssgvk_status as Status, rsf_fgvk_setup as Setup};
use crate::sys::{rsf_slvk_call, rsf_slvk_callback};
use crate::vulkan_dlss_fg::sdk_fg_options::Packed;
use crate::vulkan_dlss_fg::sdk_fg_options::State;
use crate::vulkan_fg_resources::Image;
use crate::vulkan_fg_resources::Platform as Gpu;
use crate::vulkan_streamline_camera::sdk_api::{
    FreeResources, GetFgState, SetConstants, SetOptions, SetTagForFrame,
};
use crate::vulkan_streamline_camera::{rsf_slvk_sdk_constants, rsf_slvk_sdk_tag};
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
pub(super) struct Tag {
    pub image: *const Image,
    pub kind: u32,
    pub lifetime: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct SdkCall {
    pub native: Handle,
    pub result: i32,
    pub sample: Sample,
    pub commands: Handle,
    pub camera: *const rsf_slvk_sdk_constants,
    pub record: *const Record,
    pub tags: *const Tag,
    pub options: *const Packed,
}

/// `rsf_dlgvk_sdk`: the Streamline entries at the start of the native owner.
#[repr(C)]
#[derive(Default)]
pub(super) struct Sdk {
    pub set_constants: Option<SetConstants<rsf_slvk_sdk_constants>>,
    pub set_tags: Option<SetTagForFrame<rsf_slvk_sdk_tag>>,
    pub free_resources: Option<FreeResources>,
    pub set_options: Option<SetOptions<Packed>>,
    pub get_state: Option<GetFgState<State, Packed>>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Platform {
    pub gpu: *const Gpu,
    pub open: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    pub close: unsafe extern "C" fn(Handle),
    pub prepare_chain: unsafe extern "C" fn(Handle, *mut u32, *mut Handle) -> i32,
    pub start_capture: unsafe extern "C" fn(Handle) -> i32,
    pub invoke: unsafe extern "C" fn(*const rsf_slvk_call) -> i32,
    pub initialize: rsf_slvk_callback,
    pub configure: rsf_slvk_callback,
    pub sample: rsf_slvk_callback,
    pub constants: rsf_slvk_callback,
    pub tag: rsf_slvk_callback,
    pub release: rsf_slvk_callback,
    pub capture: unsafe extern "C" fn(Handle, Handle, *const SourceImage, *const Record, u32, u64),
    pub remember: unsafe extern "C" fn(Handle, *const Record, *const Status, u32),
    pub capture_group: unsafe extern "C" fn(Handle, u64, u32),
    pub flush: unsafe extern "C" fn(Handle),
    pub detach_capture: unsafe extern "C" fn(Handle) -> i32,
    pub timeout_epoch: unsafe extern "C" fn() -> u64,
}
