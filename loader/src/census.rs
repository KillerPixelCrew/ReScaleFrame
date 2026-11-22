// SPDX-License-Identifier: MIT
#![allow(non_camel_case_types)]
// The wrapper's return address, captured in the wrapper itself before any other call frame.
macro_rules! caller {
    () => {{
        let mut address = std::ptr::null_mut();
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(
                1,
                1,
                &mut address,
                std::ptr::null_mut(),
            )
        };
        address
    }};
}

// The original function a hook saved for `$entry`, as `$signature`, or `None` when it has none.
macro_rules! original {
    ($entry:expr, $signature:ty) => {{
        let address = rsf_census_hook_original(id($entry));
        if address.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut std::ffi::c_void, $signature>(address) })
        }
    }};
}

mod d3d;
mod decode;
mod fsr;
mod hooks;
mod identity;
mod json;
mod memory;
mod misc;
mod ngx;
mod output;
mod record;
mod session;
mod static_scan;
mod stats;
mod vendor;
mod vulkan;

use crate::sys::rsf_census_platform;
use std::sync::OnceLock;

// The C leaves of loader/census/src, which this archive cannot name directly: the thin, shim and
// plugin carriers link it without them. The table is a C static for the process lifetime.
struct Platform(rsf_census_platform);
unsafe impl Send for Platform {}
unsafe impl Sync for Platform {}
static PLATFORM: OnceLock<Platform> = OnceLock::new();

fn publish(platform: &rsf_census_platform) {
    let _ = PLATFORM.set(Platform(*platform));
}

fn platform() -> &'static rsf_census_platform {
    &PLATFORM
        .get()
        .expect("the census platform is published at install")
        .0
}

const RSF_CENSUS_RECORD_BODY_MAX: usize = 6144;
const RSF_CENSUS_RECORD_STACK_FRAMES: usize = 32;
const RSF_CENSUS_RECORD_THREAD_NAME: usize = 40;

/// A JSON writer over a caller's buffer.
#[derive(Clone, Copy)]
struct rsf_census_json {
    buffer: *mut std::ffi::c_char,
    capacity: usize,
    length: usize,
    comma: u32,
    depth: u32,
    overflow: bool,
}

/// One full call record, queued for the writer thread.
struct rsf_census_queued_record {
    len: u32,
    fn_: u16,
    t_us: u64,
    present: u64,
    ord: u64,
    site_ord: u64,
    base: u64,
    ret: u64,
    rva: u32,
    tid: u32,
    stack_n: u32,
    stack: [*mut std::ffi::c_void; RSF_CENSUS_RECORD_STACK_FRAMES],
    thread: [std::ffi::c_char; RSF_CENSUS_RECORD_THREAD_NAME],
    body: [std::ffi::c_char; RSF_CENSUS_RECORD_BODY_MAX],
}

/// A wrapper's call in flight: its identity, stack and argument JSON.
struct rsf_census_call_state {
    fn_: u16,
    full: bool,
    rva: u32,
    tid: u32,
    base: u64,
    site_ord: u64,
    ord: u64,
    t_us: u64,
    stack: [*mut std::ffi::c_void; RSF_CENSUS_RECORD_STACK_FRAMES],
    stack_n: u32,
    json: rsf_census_json,
    body: [std::ffi::c_char; RSF_CENSUS_RECORD_BODY_MAX],
}

#[derive(Clone, Copy)]
struct rsf_census_sample {
    ordinal: u64,
    site_ordinal: u64,
}
