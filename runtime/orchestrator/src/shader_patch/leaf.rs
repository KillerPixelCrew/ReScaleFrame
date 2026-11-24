// SPDX-License-Identifier: MIT
//! Records the compiler, reflection and device leaves exchange with the owner and its fixtures.

use crate::sys::{rsf_patch_bytes, rsf_patch_define};
use std::ffi::{c_char, c_void};

pub(super) type Sink = Option<unsafe fn(*mut c_void, *const c_void, usize)>;
type Include = Option<unsafe fn(*mut c_void, *const c_char, *mut *const c_void, *mut usize) -> i32>;

pub(super) const RSF_PATCH_SIGNATURE_INPUT: i32 = 0;
pub(super) const RSF_PATCH_SIGNATURE_OUTPUT: i32 = 1;
#[cfg(not(test))]
pub(super) const RSF_PATCH_SIGNATURE_PATCH: i32 = 2;

#[cfg_attr(test, allow(dead_code))]
pub(super) struct CompileCall {
    #[cfg_attr(test, allow(dead_code))]
    pub compiler: *mut c_void,
    pub source: rsf_patch_bytes,
    pub defines: *const rsf_patch_define,
    pub define_count: usize,
    #[cfg_attr(test, allow(dead_code))]
    pub entry: *const c_char,
    pub target: *const c_char,
    #[cfg_attr(test, allow(dead_code))]
    pub flags: u32,
    pub user: *mut c_void,
    pub include: Include,
    pub code: Sink,
    pub errors: Sink,
}

pub(super) struct BindingRecord {
    pub name: *const c_char,
    pub type_: u32,
    pub point: u32,
    pub count: u32,
    pub return_type: u32,
    pub dimension: u32,
}

pub(super) struct ParameterRecord {
    pub semantic: *const c_char,
    pub index: u32,
    pub reg: u32,
    pub system_value: u32,
    pub component_type: u32,
    pub mask: u32,
}

#[cfg_attr(test, allow(dead_code))]
pub(super) struct ReflectionSink {
    pub user: *mut c_void,
    pub shader: Option<unsafe fn(user: *mut c_void, version: u32)>,
    pub binding: Option<unsafe fn(user: *mut c_void, binding: *const BindingRecord)>,
    #[cfg_attr(test, allow(dead_code))]
    pub buffer: Option<unsafe fn(user: *mut c_void, name: *const c_char, size: u32)>,
    #[cfg_attr(test, allow(dead_code))]
    pub variable: Option<unsafe fn(user: *mut c_void, name: *const c_char, offset: u32, size: u32)>,
    pub parameter:
        Option<unsafe fn(user: *mut c_void, signature: u32, parameter: *const ParameterRecord)>,
}
