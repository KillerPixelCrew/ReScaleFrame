// SPDX-License-Identifier: MIT
// Hand-written mirrors of rescaleframe/d3d11_host.h and the private
// d3d11_host_platform.h table. d3d11_host.cpp asserts the same sizes and
// offsets on the C++ side; the const blocks below assert them here.
#![allow(non_camel_case_types)]

use crate::auxiliary_sr_abi::{rsf_aux_platform, rsf_d3d11_auxiliary_surface};
use crate::native_sr_abi::rsf_motion_decode_params;
use crate::overlay_abi::rsf_overlay_service_setup;
use crate::render_abi::rsf_render_settings;
use crate::scene_abi::{rsf_native_present_record, rsf_observer_present_event};
use rsf_game_sys::{rsf_frame_record, rsf_game_cpu_event, rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::{c_char, c_int, c_void};
use std::mem::{offset_of, size_of};

pub(crate) const RSF_D3D11_HOST_ABI_VERSION: u32 = 8;
pub(crate) const RSF_D3D11_HOST_CAP_FG_INPUTS: u32 = 1;
pub(crate) const RSF_D3D11_HOST_CAP_INPUT_TIMELINE: u32 = 2;
pub(crate) const RSF_D3D11_HOST_CAP_RENDERED_SOURCE: u32 = 4;
pub(crate) const RSF_D3D11_CAPTURE_CONTROL_VERSION: u32 = 1;
pub(crate) const RSF_D3D11_CAPTURE_SEQUENCE: u32 = 1;
pub(crate) const RSF_D3D11_CAPTURE_FAILURES: u32 = 2;
pub(crate) const RSF_D3D11_DISPATCH_RELEASE: u32 = 1;
pub(crate) const RSF_D3D11_DISPATCH_PRESENT: u32 = 2;
pub(crate) const RSF_D3D11_DISPATCH_RESIZE_BUFFERS1: u32 = 7;

pub(crate) const RSF_D3D11_REDUCED_FRAME_ABI_VERSION: u32 = 1;
pub(crate) const RSF_D3D11_REDUCED_RESERVE: u32 = 0;
pub(crate) const RSF_D3D11_REDUCED_SEAL: u32 = 1;
pub(crate) const RSF_D3D11_REDUCED_FINISH: u32 = 2;
pub(crate) const RSF_D3D11_REDUCED_RETIRE: u32 = 3;
pub(crate) const RSF_D3D11_REDUCED_CANCEL: u32 = 4;
pub(crate) const RSF_D3D11_REDUCED_NONE: u32 = 0;
pub(crate) const RSF_D3D11_REDUCED_RESERVED: u32 = 1;
pub(crate) const RSF_D3D11_REDUCED_SEALED: u32 = 2;
pub(crate) const RSF_D3D11_REDUCED_TEMPORAL: u32 = 3;
pub(crate) const RSF_D3D11_REDUCED_SPATIAL: u32 = 4;
pub(crate) const RSF_D3D11_REDUCED_CANCELLED: u32 = 5;
pub(crate) const RSF_D3D11_REDUCED_UNSAFE: u32 = 6;
pub(crate) const RSF_D3D11_REDUCED_CLEANUP_PENDING: u32 = 7;
pub(crate) const RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED: u32 = 0x1;
pub(crate) const RSF_D3D11_REDUCED_TEMPORAL_OUTPUT: u32 = 0x2;
pub(crate) const RSF_D3D11_REDUCED_SPATIAL_OUTPUT: u32 = 0x4;
pub(crate) const RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED: u32 = 0x8;
pub(crate) const RSF_D3D11_REDUCED_CLEANUP_REQUIRED: u32 = 0x10;
pub(crate) const RSF_D3D11_REDUCED_REQUIRED_RECOVERY_FAILED: u32 = 0x20;
pub(crate) const RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED: u32 = 0x40;

pub(crate) type Pass = rsf_game_render_pass;
pub(crate) type Surface = rsf_d3d11_auxiliary_surface;
pub(crate) type AcceptWindow = Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>;
pub(crate) type PresentEvent =
    Option<unsafe extern "C" fn(*mut c_void, *const rsf_observer_present_event)>;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct rsf_d3d11_host_setup {
    pub struct_size: u32,
    pub abi_version: u32,
    pub configuration_path: *const u16,
    pub configuration_section: *const u16,
    pub runtime_directory: *const u16,
    pub identity_utf8: *const c_char,
    pub executable_sha256: *const c_char,
    pub engine_type: u32,
    pub engine_version_utf8: *const c_char,
    pub project_id_utf8: *const c_char,
    pub units_to_meters: f32,
    pub depth_inverted: u32,
    pub motion: rsf_motion_decode_params,
    pub quality_choices: u32,
    pub presentation_bridge: u32,
    pub accept_window: AcceptWindow,
    pub presentation_user: *mut c_void,
    pub capabilities: u32,
    pub colour_transport_default: u32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_present_dispatch {
    pub struct_size: u32,
    pub abi_version: u32,
    pub receiver: *mut c_void,
    pub sync_interval: u32,
    pub flags: u32,
    pub handled: u32,
    pub native_result: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union rsf_d3d11_chain_args {
    pub present: [u32; 2],
    pub raw: [u64; 5],
}

#[repr(C)]
pub(crate) struct rsf_d3d11_chain_dispatch {
    pub struct_size: u32,
    pub abi_version: u32,
    pub method: u32,
    pub handled: u32,
    pub receiver: *mut c_void,
    pub args: rsf_d3d11_chain_args,
    pub result: i32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_native_chain {
    pub struct_size: u32,
    pub abi_version: u32,
    pub device: *mut c_void,
    pub immediate_context: *mut c_void,
    pub swapchain: *mut *mut c_void,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_creation_scope {
    pub struct_size: u32,
    pub abi_version: u32,
    pub window: *mut c_void,
    pub token: u64,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_presentation_binding {
    pub struct_size: u32,
    pub abi_version: u32,
    pub swapchain: *mut c_void,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_capture_control {
    pub struct_size: u32,
    pub abi_version: u32,
    pub operation: u32,
    pub value: u32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_host_capture {
    pub struct_size: u32,
    pub abi_version: u32,
    pub session_id: u64,
    pub native_frame: u64,
    pub view_key: u64,
    pub name: *const c_char,
    pub description: *const c_char,
    pub texture: *mut c_void,
    pub view: u32,
    pub scale: f32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_auxiliary_pass {
    pub struct_size: u32,
    pub abi_version: u32,
    pub pass: *const Pass,
    pub surface_count: u32,
    pub surfaces: *const Surface,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_auxiliary_scope {
    pub struct_size: u32,
    pub abi_version: u32,
    pub session_id: u64,
    pub native_frame: u64,
    pub view_key: u64,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_reduced_frame {
    pub struct_size: u32,
    pub abi_version: u32,
    pub operation: u32,
    pub disposition: u32,
    pub token: u64,
    pub owner: *const Pass,
    pub surface_count: u32,
    pub reserved: u32,
    pub surfaces: *const Surface,
    pub temporal_result: i32,
    pub result_flags: u32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11_sampler_bias {
    pub struct_size: u32,
    pub abi_version: u32,
    pub session_id: u64,
    pub samplers: [*mut c_void; 16],
    pub slot_mask: u32,
    pub bias: f32,
    pub applied_mask: u32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11h_strings {
    pub streamline: *const c_char,
    pub fidelityfx: *const c_char,
    pub xess: *const c_char,
    pub captures: *const c_char,
    pub identity: *const c_char,
    pub fingerprint: *const c_char,
    pub engine_version: *const c_char,
    pub project: *const c_char,
}

#[repr(C)]
pub(crate) struct rsf_d3d11h_bridge {
    pub mode: u32,
    pub rendered: u32,
    pub allowed_backend_choices: u32,
    pub max_generated_frames: u32,
    pub setup: *const rsf_d3d11_host_setup,
    pub strings: *const rsf_d3d11h_strings,
    pub log: rsf_game_log_fn,
    pub user: *mut c_void,
    pub accept_window: AcceptWindow,
    pub present_event: PresentEvent,
}

#[repr(C)]
pub(crate) struct rsf_d3d11h_pipeline_start {
    pub output_width: u32,
    pub output_height: u32,
    pub quality: u32,
    pub reserved: u32,
    pub setup: *const rsf_d3d11_host_setup,
    pub strings: *const rsf_d3d11h_strings,
    pub log: rsf_game_log_fn,
    pub user: *mut c_void,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct rsf_d3d11h_pipeline_status {
    pub running: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub render_width: u32,
    pub render_height: u32,
}

type Raw = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct rsf_d3d11h_platform {
    pub same_object: unsafe extern "C" fn(Raw, Raw) -> i32,
    pub child_device: unsafe extern "C" fn(Raw) -> Raw,
    pub is_immediate: unsafe extern "C" fn(Raw) -> u32,
    pub device_ok: unsafe extern "C" fn(Raw) -> u32,
    pub immediate_context: unsafe extern "C" fn(Raw) -> Raw,
    pub device_vendor: unsafe extern "C" fn(Raw, *mut u32) -> i32,
    pub chain_device: unsafe extern "C" fn(Raw) -> Raw,
    pub chain_window: unsafe extern "C" fn(Raw) -> Raw,
    pub chain_buffer: unsafe extern "C" fn(Raw) -> Raw,
    pub install_bridge: unsafe extern "C" fn(*const rsf_d3d11h_bridge) -> i32,
    pub observer_install: unsafe extern "C" fn(rsf_game_log_fn, Raw, PresentEvent) -> i32,
    pub present_is_owner: unsafe extern "C" fn(Raw) -> i32,
    pub chain_dispatch: unsafe extern "C" fn(*mut rsf_d3d11_chain_dispatch),
    pub creation_begin: unsafe extern "C" fn(Raw, *mut u64) -> i32,
    pub creation_end: unsafe extern "C" fn(Raw, u64, Raw, Raw, Raw) -> i32,
    pub wrap_chain:
        unsafe extern "C" fn(*const rsf_d3d11_native_chain, rsf_game_log_fn, Raw) -> i32,
    pub fg_frame: unsafe extern "C" fn(*const Pass, u32),
    pub fg_submission: unsafe extern "C" fn(*const Pass, u32),
    pub fg_final: unsafe extern "C" fn(Raw, *const Pass),
    pub fg_capture: unsafe extern "C" fn(Raw, *const Pass, *const rsf_motion_decode_params) -> i32,
    pub scene_pass: unsafe extern "C" fn(*const Pass, u32) -> i32,
    pub scene_present_path: unsafe extern "C" fn(*const Pass) -> u32,
    pub window_scope: unsafe extern "C" fn(*const Pass, u32) -> i32,
    pub window_present: unsafe extern "C" fn(*const rsf_observer_present_event) -> i32,
    pub window_last_present: unsafe extern "C" fn(*mut rsf_native_present_record) -> i32,
    pub window_match: unsafe extern "C" fn(Raw, *mut Pass) -> i32,
    pub latency_window_bind: unsafe extern "C" fn(u64, Raw),
    pub cpu_dispatch_fg: unsafe extern "C" fn(*const rsf_game_cpu_event) -> i32,
    pub cpu_event: unsafe extern "C" fn(*const rsf_game_cpu_event),
    pub dlss_native_present: unsafe extern "C" fn(Raw, u32, u32, u32) -> i32,
    pub pipeline_start: unsafe extern "C" fn(Raw, *const rsf_d3d11h_pipeline_start) -> i32,
    pub pipeline_resize: unsafe extern "C" fn(u32, u32) -> i32,
    pub pipeline_select: unsafe extern "C" fn(u32) -> i32,
    pub pipeline_quality: unsafe extern "C" fn(u32) -> i32,
    pub pipeline_preset: unsafe extern "C" fn(u32) -> i32,
    pub pipeline_sharpen: unsafe extern "C" fn(*const rsf_render_settings),
    pub pipeline_status: unsafe extern "C" fn(*mut rsf_d3d11h_pipeline_status) -> i32,
    pub pipeline_colour: unsafe extern "C" fn(u32, u32),
    pub pipeline_output: unsafe extern "C" fn() -> Raw,
    pub sr_evaluate: unsafe extern "C" fn(Raw, *const Pass) -> i32,
    pub sr_destinations_ordered: unsafe extern "C" fn() -> i32,
    pub dump_configure: unsafe extern "C" fn(*const rsf_d3d11h_strings, rsf_game_log_fn, Raw),
    pub dump_request: unsafe extern "C" fn(),
    pub dump_requested: unsafe extern "C" fn() -> i32,
    pub dump_poll: unsafe extern "C" fn(),
    pub dump_begin: unsafe extern "C" fn(*const rsf_frame_record, u32) -> i32,
    pub dump_wants: unsafe extern "C" fn(*const rsf_frame_record) -> i32,
    pub dump_inputs: unsafe extern "C" fn(Raw, *const Pass, *const rsf_frame_record, Raw),
    pub dump_pass: unsafe extern "C" fn(Raw, *const Pass, u32),
    pub dump_texture: unsafe extern "C" fn(
        Raw,
        *const rsf_frame_record,
        *const c_char,
        *const c_char,
        Raw,
        u32,
        f32,
    ) -> i32,
    pub dump_result: unsafe extern "C" fn(*const rsf_frame_record, *const c_char, u32, i32),
    pub dump_complete: unsafe extern "C" fn(*const rsf_frame_record, i32),
    pub overlay_configure:
        unsafe extern "C" fn(*const rsf_overlay_service_setup, *mut rsf_render_settings) -> i32,
    pub overlay_present: unsafe extern "C" fn(Raw),
    pub overlay_poll_toggle: unsafe extern "C" fn(),
    pub ini_read: unsafe extern "C" fn(*const u16, *const u16, *const u16, i32) -> i32,
    pub ini_write: unsafe extern "C" fn(*const u16, *const u16, *const u16, *const u16) -> i32,
    pub failure_claim: unsafe extern "C" fn(u32, u32, i32) -> i32,
    pub sequence_request: unsafe extern "C" fn(),
    pub sequence_claim: unsafe extern "C" fn(u64, u64, u32, u32) -> i32,
    pub auxiliary: *const rsf_aux_platform,
}

const _: () = {
    assert!(size_of::<rsf_d3d11_host_setup>() == 160);
    assert!(offset_of!(rsf_d3d11_host_setup, engine_type) == 48);
    assert!(offset_of!(rsf_d3d11_host_setup, units_to_meters) == 72);
    assert!(offset_of!(rsf_d3d11_host_setup, motion) == 80);
    assert!(offset_of!(rsf_d3d11_host_setup, quality_choices) == 128);
    assert!(offset_of!(rsf_d3d11_host_setup, accept_window) == 136);
    assert!(offset_of!(rsf_d3d11_host_setup, capabilities) == 152);
    assert!(size_of::<rsf_d3d11_present_dispatch>() == 32);
    assert!(offset_of!(rsf_d3d11_present_dispatch, native_result) == 28);
    assert!(size_of::<rsf_d3d11_chain_dispatch>() == 72);
    assert!(offset_of!(rsf_d3d11_chain_dispatch, receiver) == 16);
    assert!(offset_of!(rsf_d3d11_chain_dispatch, args) == 24);
    assert!(offset_of!(rsf_d3d11_chain_dispatch, result) == 64);
    assert!(size_of::<rsf_d3d11_native_chain>() == 32);
    assert!(size_of::<rsf_d3d11_creation_scope>() == 24);
    assert!(size_of::<rsf_d3d11_presentation_binding>() == 16);
    assert!(size_of::<rsf_d3d11_capture_control>() == 16);
    assert!(size_of::<rsf_d3d11_host_capture>() == 64);
    assert!(offset_of!(rsf_d3d11_host_capture, view) == 56);
    assert!(size_of::<rsf_d3d11_auxiliary_pass>() == 32);
    assert!(offset_of!(rsf_d3d11_auxiliary_pass, surfaces) == 24);
    assert!(size_of::<rsf_d3d11_auxiliary_scope>() == 32);
    assert!(size_of::<rsf_d3d11_reduced_frame>() == 56);
    assert!(offset_of!(rsf_d3d11_reduced_frame, token) == 16);
    assert!(offset_of!(rsf_d3d11_reduced_frame, owner) == 24);
    assert!(offset_of!(rsf_d3d11_reduced_frame, surfaces) == 40);
    assert!(offset_of!(rsf_d3d11_reduced_frame, temporal_result) == 48);
    assert!(size_of::<rsf_d3d11_sampler_bias>() == 160);
    assert!(offset_of!(rsf_d3d11_sampler_bias, slot_mask) == 144);
    assert!(offset_of!(rsf_d3d11_sampler_bias, applied_mask) == 152);
    assert!(size_of::<rsf_d3d11h_strings>() == 64);
    assert!(size_of::<rsf_d3d11h_bridge>() == 64);
    assert!(size_of::<rsf_d3d11h_pipeline_start>() == 48);
    assert!(size_of::<rsf_d3d11h_pipeline_status>() == 20);
    assert!(size_of::<rsf_d3d11h_platform>() == 61 * 8);
};
