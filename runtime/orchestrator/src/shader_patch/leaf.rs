// SPDX-License-Identifier: MIT
//! Internal compiler, reflection and device dispatch shared with owner fixtures.

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

pub(super) struct Platform {

    pub compile: unsafe fn(call: *const CompileCall) -> i32,
    pub reflect: unsafe fn(code: rsf_patch_bytes, sink: *const ReflectionSink) -> i32,
    pub disassemble: unsafe fn(code: rsf_patch_bytes, user: *mut c_void, text: Sink) -> i32,
    pub create_shader: unsafe fn(
        device: *mut c_void,
        stage: u32,
        code: rsf_patch_bytes,
        shader: *mut *mut c_void,
    ) -> i32,
    pub create_constant_buffer:
        unsafe fn(device: *mut c_void, size: u32, buffer: *mut *mut c_void) -> i32,
    pub buffer_desc:
        unsafe fn(buffer: *mut c_void, width: *mut u32, usage: *mut u32, cpu_access: *mut u32),
    pub upload: unsafe fn(context: *mut c_void, buffer: *mut c_void, data: rsf_patch_bytes) -> i32,
    pub bind_constant: unsafe fn(context: *mut c_void, stage: u32, slot: u32, buffer: *mut c_void),
    pub hooks_ready: unsafe fn() -> i32,
    pub hook_create:
        unsafe fn(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32,
    pub hook_enable: unsafe fn(target: *mut c_void) -> i32,
    pub hook_disable: unsafe fn(target: *mut c_void),
    pub hook_remove: unsafe fn(target: *mut c_void),
}
