// SPDX-License-Identifier: MIT
// Records the presentation owner and its native leaves exchange. Rust-only, except the Streamline
// graphics record from rescaleframe/streamline_host.h.
#![allow(non_camel_case_types)]

pub(crate) use crate::sys::rsf_streamline_graphics;
use std::ffi::c_void;

pub(crate) type Raw = *mut c_void;

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
