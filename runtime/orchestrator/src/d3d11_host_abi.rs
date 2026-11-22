// SPDX-License-Identifier: MIT
// Hand-written mirrors of rescaleframe/d3d11_host.h and the private
// d3d11_host_platform.h table. d3d11_host.cpp asserts the same sizes and
// offsets on the C++ side; the const blocks below assert them here.
#![allow(non_camel_case_types)]

use crate::auxiliary_sr_abi::rsf_aux_platform;
use crate::fg12_abi::{rsf_d3d11_present_setup, rsf_streamline_host};
use crate::native_sr_abi::rsf_motion_decode_params;
use crate::overlay_abi::rsf_overlay_service_setup;
use crate::render_abi::{rsf_backend_log_fn, rsf_native_fg_options, rsf_render_settings};
use crate::scene_abi::{rsf_native_present_record, rsf_observer_present_event};
pub(crate) use rsf_game_sys::d3d11_chain_abi::*;
use rsf_game_sys::d3d11_host_abi::rsf_d3d11_auxiliary_surface;
pub(crate) use rsf_game_sys::d3d11_host_abi::{
    rsf_d3d11_host_setup, rsf_d3d11_native_chain, rsf_d3d11_creation_scope,
    rsf_d3d11_auxiliary_pass, rsf_d3d11_auxiliary_scope, rsf_d3d11_host_capture,
    rsf_d3d11_sampler_bias,
};
use rsf_game_sys::{rsf_frame_record, rsf_game_cpu_event, rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::{c_char, c_int, c_void};
use std::mem::{offset_of, size_of};

pub(crate) const RSF_D3D11_HOST_CAP_FG_INPUTS: u32 = 1;
pub(crate) const RSF_D3D11_HOST_CAP_INPUT_TIMELINE: u32 = 2;
pub(crate) const RSF_D3D11_HOST_CAP_RENDERED_SOURCE: u32 = 4;
pub(crate) const RSF_D3D11_CAPTURE_CONTROL_VERSION: u32 = 1;
pub(crate) const RSF_D3D11_CAPTURE_SEQUENCE: u32 = 1;
pub(crate) const RSF_D3D11_CAPTURE_FAILURES: u32 = 2;

pub(crate) use rsf_game_sys::d3d11_reduced_abi::{
    RSF_D3D11_REDUCED_CANCEL, RSF_D3D11_REDUCED_CANCELLED, RSF_D3D11_REDUCED_CLEANUP_PENDING,
    RSF_D3D11_REDUCED_CLEANUP_REQUIRED, RSF_D3D11_REDUCED_FINISH,
    RSF_D3D11_REDUCED_FRAME_ABI_VERSION, RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED,
    RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED, RSF_D3D11_REDUCED_NONE,
    RSF_D3D11_REDUCED_REQUIRED_RECOVERY_FAILED, RSF_D3D11_REDUCED_RESERVE,
    RSF_D3D11_REDUCED_RESERVED, RSF_D3D11_REDUCED_RETIRE, RSF_D3D11_REDUCED_SEAL,
    RSF_D3D11_REDUCED_SEALED, RSF_D3D11_REDUCED_SPATIAL, RSF_D3D11_REDUCED_SPATIAL_OUTPUT,
    RSF_D3D11_REDUCED_TEMPORAL, RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED,
    RSF_D3D11_REDUCED_TEMPORAL_OUTPUT, RSF_D3D11_REDUCED_UNSAFE, rsf_d3d11_reduced_frame,
};

pub(crate) type Pass = rsf_game_render_pass;
pub(crate) type Surface = rsf_d3d11_auxiliary_surface;
pub(crate) type PresentEvent =
    Option<unsafe extern "C" fn(*mut c_void, *const rsf_observer_present_event)>;

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
pub(crate) struct rsf_d3d11h_pipeline_start {
    pub output_width: u32,
    pub output_height: u32,
    pub quality: u32,
    pub backend: u32,
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
    pub present_install: unsafe extern "C" fn(*const rsf_d3d11_present_setup) -> c_int,
    pub present_prepare: unsafe extern "C" fn(*const rsf_d3d11_present_setup) -> c_int,
    pub present_install_scoped: unsafe extern "C" fn(*const rsf_d3d11_present_setup) -> c_int,
    pub fg_prepare:
        unsafe extern "C" fn(Raw, Raw, Raw, Raw, Raw, *mut rsf_streamline_host, Raw, u32),
    pub fg_retire: unsafe extern "C" fn(Raw, Raw),
    pub fg_latency_event: unsafe extern "C" fn(Raw, *const rsf_observer_present_event),
    pub fg_rendered_source_set: unsafe extern "C" fn(u32),
    pub fg_set_log: unsafe extern "C" fn(rsf_backend_log_fn, Raw),
    pub fg_options_set: unsafe extern "C" fn(*const rsf_native_fg_options),
    pub observer_install: unsafe extern "C" fn(rsf_game_log_fn, Raw, PresentEvent) -> i32,
    pub present_is_owner: unsafe extern "C" fn(Raw) -> i32,
    pub chain_dispatch: unsafe extern "C" fn(*mut rsf_d3d11_chain_dispatch),
    pub creation_begin: unsafe extern "C" fn(Raw, *mut u64) -> i32,
    pub creation_end: unsafe extern "C" fn(Raw, u64, Raw, Raw, Raw) -> i32,
    pub wrap_native: unsafe extern "C" fn(Raw, Raw, *mut Raw) -> i32,
    pub com_identity: unsafe extern "C" fn(Raw) -> Raw,
    pub chain_present: unsafe extern "C" fn(Raw) -> Raw,
    pub com_release: unsafe extern "C" fn(Raw) -> u32,
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
    pub sr_evaluate: unsafe extern "C" fn(Raw, *const Pass, *mut u32) -> i32,
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
    pub auxiliary: *const rsf_aux_platform,
}

const _: () = {
    assert!(size_of::<rsf_d3d11_host_setup>() == 168);
    assert!(offset_of!(rsf_d3d11_host_setup, engine_type) == 48);
    assert!(offset_of!(rsf_d3d11_host_setup, units_to_meters) == 72);
    assert!(offset_of!(rsf_d3d11_host_setup, motion) == 80);
    assert!(offset_of!(rsf_d3d11_host_setup, quality_choices) == 128);
    assert!(offset_of!(rsf_d3d11_host_setup, accept_window) == 136);
    assert!(offset_of!(rsf_d3d11_host_setup, capabilities) == 152);
    assert!(offset_of!(rsf_d3d11_host_setup, depth_far_plane) == 160);
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
    assert!(size_of::<rsf_d3d11h_pipeline_start>() == 48);
    assert!(size_of::<rsf_d3d11h_pipeline_status>() == 20);
    assert!(size_of::<rsf_d3d11h_platform>() == 68 * 8);
    assert!(offset_of!(rsf_d3d11h_platform, present_install) == 9 * 8);
    assert!(offset_of!(rsf_d3d11h_platform, fg_options_set) == 17 * 8);
    assert!(offset_of!(rsf_d3d11h_platform, wrap_native) == 23 * 8);
    assert!(offset_of!(rsf_d3d11h_platform, com_release) == 26 * 8);
    assert!(offset_of!(rsf_d3d11h_platform, auxiliary) == 67 * 8);
};
