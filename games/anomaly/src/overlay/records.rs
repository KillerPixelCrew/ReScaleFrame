// SPDX-License-Identifier: MIT
use std::ffi::{c_char, c_void};

// Adapter options are read and applied by the Present owner.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct rsf_anomaly_overlay_option {
    pub(crate) label: *const c_char,
    pub(crate) toggle: u32,
    pub(crate) restart: u32,
    pub(crate) minimum: i32,
    pub(crate) maximum: i32,
    pub(crate) step: i32,
    pub(crate) divisor: u32,
    pub(crate) get: Option<unsafe extern "C" fn() -> i32>,
    pub(crate) set: Option<unsafe extern "C" fn(i32) -> bool>,
}

// The borrowed D3D11 device address is used only as an identity.
#[derive(Copy, Clone)]
pub(crate) struct ChainInfo {
    pub(crate) window: *mut c_void,
    pub(crate) device: *const c_void,
    pub(crate) width: u32,
    pub(crate) height: u32,
}
