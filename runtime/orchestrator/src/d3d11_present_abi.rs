// SPDX-License-Identifier: MIT
// Hand-written mirror of runtime/presentation/src/d3d11_present_platform.h. The C++
// facade asserts the same sizes and offsets with static_assert.
#![allow(non_camel_case_types)]

use crate::render_abi::rsf_generation_provider;
use crate::scene_abi::rsf_observer_present_event;
use std::ffi::{c_char, c_void};
use std::mem::offset_of;

pub(crate) type Raw = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct rsf_streamline_graphics {
    pub struct_size: u32,
    pub device: Raw,
    pub queue: Raw,
    pub factory: Raw,
    pub native_device: Raw,
    pub native_queue: Raw,
}

impl rsf_streamline_graphics {
    pub(crate) const EMPTY: Self = Self {
        struct_size: 0,
        device: std::ptr::null_mut(),
        queue: std::ptr::null_mut(),
        factory: std::ptr::null_mut(),
        native_device: std::ptr::null_mut(),
        native_queue: std::ptr::null_mut(),
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct rsf_d3d11p_request {
    pub window: Raw,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub sample_count: u32,
    pub buffer_count: u32,
    pub windowed: u32,
    pub flags: u32,
    pub swap_effect: u32,
    pub refresh_numerator: u32,
    pub refresh_denominator: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct rsf_d3d11p_facts {
    pub window: Raw,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub buffer_count: u32,
    pub flags: u32,
    pub windowed: u32,
    pub native12: u32,
    pub tearing: u32,
    pub graphics: rsf_streamline_graphics,
    pub direct_queue: Raw,
    pub interop_queue: Raw,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct rsf_d3d11p_adapter {
    pub vendor: u32,
    pub device: u32,
    pub default_backend: u32,
    pub reserved: u32,
}

pub(crate) const FAMILY_DLSS: u32 = 1;
pub(crate) const FAMILY_FIDELITYFX: u32 = 2;
pub(crate) const FAMILY_XESS: u32 = 3;
pub(crate) const JOIN_SOURCE_TO_PRESENT: u32 = 0;
pub(crate) const JOIN_PRESENT_TO_SOURCE: u32 = 1;
pub(crate) const DRAIN_GRAPHICS: u32 = 1;
pub(crate) const DRAIN_INTEROP: u32 = 2;
pub(crate) const BUFFERS_SHARED: u32 = 0;
pub(crate) const BUFFERS_RENDER: u32 = 1;

pub(crate) enum Chain {}

#[repr(C)]
pub(crate) struct rsf_d3d11p_platform {
    pub provider_table: unsafe extern "C" fn(u32) -> *const rsf_generation_provider,
    pub observer_present: unsafe extern "C" fn(*mut rsf_observer_present_event),
    pub window_valid: unsafe extern "C" fn(Raw) -> u32,
    pub client_size: unsafe extern "C" fn(Raw, *mut u32, *mut u32) -> u32,
    pub canonical_identity: unsafe extern "C" fn(Raw) -> Raw,
    pub add_ref: unsafe extern "C" fn(Raw),
    pub release: unsafe extern "C" fn(Raw),
    pub host_sleep: unsafe extern "C" fn(Raw, u64) -> i32,
    pub host_marker: unsafe extern "C" fn(Raw, u64, u32, u32) -> i32,
    pub host_acquire: unsafe extern "C" fn(Raw, u64) -> i32,
    pub host_input: unsafe extern "C" fn(Raw, u64, u32, u32) -> i32,
    pub host_abort: unsafe extern "C" fn(Raw, u64) -> i32,
    pub host_reflex_off: unsafe extern "C" fn(Raw) -> i32,
    pub host_generation_load: unsafe extern "C" fn(Raw, u32) -> i32,
    pub host_present_common: unsafe extern "C" fn(Raw, Raw, u32, u32, u32) -> i32,

    pub bridge_new: unsafe extern "C" fn(*mut Chain) -> Raw,
    pub bridge_release_owned: unsafe extern "C" fn(Raw),
    pub bridge_chain: unsafe extern "C" fn(Raw) -> *mut Chain,
    pub facts: unsafe extern "C" fn(Raw, *mut rsf_d3d11p_facts),

    pub init_source: unsafe extern "C" fn(Raw, Raw, Raw, *const c_void) -> u32,
    pub owning_adapter: unsafe extern "C" fn(Raw, Raw, *mut rsf_d3d11p_adapter) -> i32,
    pub native_device: unsafe extern "C" fn(Raw) -> i32,
    pub adopt_host: unsafe extern "C" fn(Raw, u32, *mut i32) -> u32,
    pub d3d11_device: unsafe extern "C" fn(Raw, Raw) -> i32,
    pub create_host: unsafe extern "C" fn(Raw) -> i32,
    pub host: unsafe extern "C" fn(Raw) -> Raw,
    pub resolve_graphics: unsafe extern "C" fn(Raw) -> i32,
    pub tearing_supported: unsafe extern "C" fn(Raw) -> u32,
    pub fsr4_supported: unsafe extern "C" fn(Raw, *const c_char) -> u32,
    pub create_queues: unsafe extern "C" fn(Raw, u32) -> i32,
    pub create_shared_inputs: unsafe extern "C" fn(Raw) -> i32,
    pub create_slots: unsafe extern "C" fn(Raw) -> i32,
    pub make_render_buffers: unsafe extern "C" fn(Raw) -> i32,

    pub create_plain: unsafe extern "C" fn(Raw) -> i32,
    pub adopt_chain: unsafe extern "C" fn(Raw, Raw, Raw) -> i32,
    pub physical: unsafe extern "C" fn(Raw) -> Raw,
    pub clear_physical: unsafe extern "C" fn(Raw),
    pub get_fullscreen: unsafe extern "C" fn(Raw, *mut i32) -> i32,
    pub set_fullscreen: unsafe extern "C" fn(Raw, i32, Raw) -> i32,
    pub present_test: unsafe extern "C" fn(Raw, u32, u32) -> i32,
    pub physical_present: unsafe extern "C" fn(Raw, u32, u32) -> i32,
    pub resize_physical: unsafe extern "C" fn(Raw, u32, u32, u32, u32) -> i32,

    pub has_provider_queue: unsafe extern "C" fn(Raw) -> u32,
    pub queues_joined: unsafe extern "C" fn(Raw) -> u32,
    pub join: unsafe extern "C" fn(Raw, u32, u64) -> i32,
    pub slot_reset: unsafe extern "C" fn(Raw, u32, u64, *mut u32) -> i32,
    pub upload_wait: unsafe extern "C" fn(Raw, u64) -> i32,
    pub acquire_backbuffer: unsafe extern "C" fn(Raw) -> i32,
    pub has_render_buffer: unsafe extern "C" fn(Raw, u32) -> u32,
    pub record_copy: unsafe extern "C" fn(Raw, u32, u32),
    pub slot_command: unsafe extern "C" fn(Raw, u32) -> Raw,
    pub backbuffer: unsafe extern "C" fn(Raw) -> Raw,
    pub context11: unsafe extern "C" fn(Raw) -> Raw,
    pub close_execute: unsafe extern "C" fn(Raw, u32, Raw) -> i32,
    pub upload_return: unsafe extern "C" fn(Raw, u64) -> i32,
    pub signal_slot: unsafe extern "C" fn(Raw, u64) -> i32,
    pub release_backbuffer: unsafe extern "C" fn(Raw),
    pub has_application_ready: unsafe extern "C" fn(Raw) -> u32,
    pub application_ready: unsafe extern "C" fn(Raw, u32, u64) -> i32,
    pub report_device_failure: unsafe extern "C" fn(Raw, i32, u32),
    pub log_present_failure: unsafe extern "C" fn(Raw, *const c_char, i32, u32),

    pub drain_queues: unsafe extern "C" fn(Raw) -> u32,
    pub signal_wait: unsafe extern "C" fn(Raw, u32, u64) -> i32,
    pub wait_fence: unsafe extern "C" fn(Raw, Raw, u64) -> i32,
    pub buffers_referenced: unsafe extern "C" fn(Raw, u32) -> u32,
    pub remake_surfaces: unsafe extern "C" fn(Raw, u32, u32, u32, u32) -> i32,
    pub set_description: unsafe extern "C" fn(Raw, u32, u32, u32, u32),
    pub remake_render_buffers: unsafe extern "C" fn(Raw, u32) -> i32,
    pub retire_fence: unsafe extern "C" fn(Raw) -> Raw,
    pub retire_submit: unsafe extern "C" fn(Raw, Raw, u64, Raw, u64) -> i32,
    pub retain_all: unsafe extern "C" fn(Raw),
    pub release_presentation: unsafe extern "C" fn(Raw),
    pub release_device: unsafe extern "C" fn(Raw, u32),
    pub presentation_queue: unsafe extern "C" fn(Raw) -> Raw,
    pub source_queue: unsafe extern "C" fn(Raw) -> Raw,
    pub command_list: unsafe extern "C" fn(Raw, Raw) -> Raw,
    pub verify_outer: unsafe extern "C" fn(Raw, Raw, Raw, Raw, Raw) -> i32,
}

const _: () = {
    assert!(size_of::<rsf_streamline_graphics>() == 48);
    assert!(size_of::<rsf_d3d11p_request>() == 48);
    assert!(offset_of!(rsf_d3d11p_request, refresh_denominator) == 44);
    assert!(size_of::<rsf_d3d11p_facts>() == 104);
    assert!(offset_of!(rsf_d3d11p_facts, graphics) == 40);
    assert!(offset_of!(rsf_d3d11p_facts, direct_queue) == 88);
    assert!(size_of::<rsf_d3d11p_adapter>() == 16);
    assert!(size_of::<rsf_d3d11p_platform>() == 77 * 8);
    assert!(offset_of!(rsf_d3d11p_platform, bridge_new) == 15 * 8);
    assert!(offset_of!(rsf_d3d11p_platform, init_source) == 19 * 8);
    assert!(offset_of!(rsf_d3d11p_platform, create_plain) == 33 * 8);
    assert!(offset_of!(rsf_d3d11p_platform, has_provider_queue) == 42 * 8);
    assert!(offset_of!(rsf_d3d11p_platform, drain_queues) == 61 * 8);
    assert!(offset_of!(rsf_d3d11p_platform, verify_outer) == 76 * 8);
};
