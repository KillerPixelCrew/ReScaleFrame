// SPDX-License-Identifier: MIT
// Records the presentation owner and its native leaves exchange. Rust-only; the streamline
// graphics record mirrors rescaleframe/streamline_host.h.
#![allow(non_camel_case_types)]

use std::ffi::c_void;

pub(crate) type Raw = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
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
    pub device11: Raw,
    pub buffer_usage: u32,
}

#[repr(C)]
pub(crate) struct rsf_d3d11p_plain_plan {
    pub queue: Raw,
    pub swap_effect: u32,
    pub flags: u32,
    pub buffer_count: u32,
    pub buffer_usage: u32,
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

pub(crate) enum Chain {}

const _: () = assert!(size_of::<rsf_streamline_graphics>() == 48);
