// SPDX-License-Identifier: MIT
use super::*;
use crate::frame_capture12::{self as capture, Owner as Capture};
use rsf_game_sys::rsf_frame_allows_fg;
use std::sync::atomic::AtomicPtr;

// The linked native calls `native!` reaches by name. Unit tests do not build this module.
pub(super) use crate::d3d11_present::{
    rsf_d3d11_present_backend as present_backend,
    rsf_d3d11_present_generation as present_generation,
    rsf_d3d11_present_has_owner as has_owner, rsf_d3d11_present_is_owner as is_owner,
    rsf_d3d11_present_provider as present_provider, rsf_d3d11_present_queue as present_queue,
    rsf_d3d11_present_retire as present_retire,
    rsf_d3d11_present_source_queue as source_queue,
};
pub(super) use crate::d3d12_host::fg::{
    rsf_fg12_platform_buffer_index as buffer_index, rsf_fg12_platform_copy as copy,
    rsf_fg12_platform_fence as fence, rsf_fg12_platform_signal as signal,
    rsf_fg12_platform_texture as texture, rsf_fg12_platform_texture_desc as texture_desc,
    rsf_fg12_platform_transition as transition,
};
pub(super) use crate::d3d12_host::pass_depth_domains as depth_domains;
pub(super) use crate::frame_capture12::native::{
    rsf_fg12_platform_map as map, rsf_fg12_platform_readback as readback,
    rsf_fg12_platform_readback_copy as readback_copy, rsf_fg12_platform_unmap as unmap,
};
pub(super) use crate::input_dump::abi::{
    rsf_input_dump_begin_record as dump_begin_record, rsf_input_dump_complete as dump_complete,
    rsf_input_dump_current as dump_current, rsf_input_dump_poll as dump_poll,
    rsf_input_dump_requested as dump_requested, rsf_input_dump_result as dump_result,
};
use crate::input_dump::rsf_input_dump_fg_prepare;
pub(super) use crate::native_cpu::rsf_native_cpu_dispatch as cpu_dispatch;
pub(super) use crate::native_fg::{
    rsf_native_fg_options_set as fg_options_set, rsf_native_fg_set_log as fg_set_log,
};
use crate::native_fg_cpu::platform::rsf_native_latency_ping;
pub(super) use crate::native_fg_cpu::platform::{
    rsf_native_fg_cpu as fg_cpu, rsf_native_latency_end_session as latency_end_session,
    rsf_native_latency_frame_get as latency_frame_get,
    rsf_native_latency_graphics as latency_graphics,
    rsf_native_latency_present as latency_present, rsf_native_latency_render as latency_render,
    rsf_native_latency_status_get as latency_status_get,
};

pub(super) unsafe fn allows_fg(record: *const rsf_frame_record) -> u32 {
    unsafe { rsf_frame_allows_fg(record) as u32 }
}

pub(super) unsafe fn dump_prepare(frame: *const rsf_fg_frame, result: i32) {
    unsafe { rsf_input_dump_fg_prepare(std::ptr::null(), frame, result) };
}

// Explicit creation only prepares the bridge. Both entries are non-unwinding
// Rust, and their native hook callbacks contain their own exceptions, so no
// foreign unwind can reach this frame.
pub(super) unsafe fn install(setup: *const rsf_d3d11_present_setup, explicit: i32) -> i32 {
    unsafe {
        if explicit != 0 {
            crate::d3d11_present::rsf_d3d11_present_prepare(setup)
        } else {
            crate::d3d11_present::rsf_d3d11_present_install(setup)
        }
    }
}

pub(super) unsafe fn host_abort(host: *mut rsf_streamline_host, source: u64) -> i32 {
    unsafe { crate::streamline_host::rsf_streamline_host_abort(host.cast(), source) }
}
// The native lifetime hooks run at module attach and detach, as the former
// C++ capture member did. Detach requires a quiescent Runtime.
static CAPTURE: AtomicPtr<Capture> = AtomicPtr::new(null_mut());

#[unsafe(no_mangle)]
extern "C" fn rsf_fg12_capture_attach() {
    CAPTURE.store(capture::rsf_core_capture12_create(1), Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg12_capture_detach() {
    let owner = CAPTURE.swap(null_mut(), Ordering::AcqRel);
    unsafe { capture::rsf_core_capture12_destroy(owner) };
}

pub(super) unsafe fn dump_record(
    list: *mut c_void,
    source: *mut c_void,
    state: u32,
    record: *const rsf_frame_record,
    role: *const c_char,
    slot: u32,
) {
    unsafe {
        capture::rsf_core_capture12_record(
            CAPTURE.load(Ordering::Acquire),
            list,
            source,
            state,
            record,
            role,
            slot,
            0,
            u32::MAX,
            u32::MAX,
            0.0,
        );
    }
}

pub(super) unsafe fn dump_flush() {
    unsafe { capture::rsf_core_capture12_flush(CAPTURE.load(Ordering::Acquire), u32::MAX, 1) };
}

pub(super) unsafe fn dump_submitted(queue: *mut c_void, record: *const rsf_frame_record) {
    unsafe {
        capture::rsf_core_capture12_submitted(
            CAPTURE.load(Ordering::Acquire),
            queue,
            u32::MAX,
            record,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_install(request: *const rsf_fg12_setup) -> i32 {
    unsafe { rsf_core_fg12_install(request) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_install_explicit(request: *const rsf_fg12_setup) -> i32 {
    unsafe { rsf_core_fg12_install_explicit(request) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_cpu(event: *const rsf_game_cpu_event) {
    unsafe { rsf_core_fg12_cpu(event) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_latency(event: *const rsf_game_latency_event) -> i32 {
    unsafe { rsf_core_fg12_latency(event) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg12_render_begin(session: u64, id: u64) -> i32 {
    rsf_core_fg12_render_begin(session, id)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_capture(
    list: *mut c_void,
    record: *const rsf_frame_record,
    depth: *mut c_void,
    motion: *mut c_void,
) {
    unsafe { rsf_core_fg12_capture(list, record, depth, motion) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_hudless(list: *mut c_void, pass: *const rsf_game_render_pass) {
    unsafe { rsf_core_fg12_hudless(list, pass) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_submitted(session: u64, id: u64) {
    unsafe { rsf_core_fg12_submitted(session, id) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg12_end_session(session: u64) -> i32 {
    rsf_core_fg12_end_session(session)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_window(pass: *const rsf_game_render_pass) {
    unsafe { rsf_core_fg12_window(pass) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_status(output: *mut rsf_native_fg_status) -> i32 {
    unsafe { rsf_core_fg12_status(output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_status_try(output: *mut rsf_native_fg_status) -> i32 {
    unsafe { rsf_core_fg12_status_try(output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_options(request: *const rsf_native_fg_options) {
    unsafe { rsf_core_fg12_options(request) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg12_capture_outputs(directory: *const c_char) -> i32 {
    unsafe { rsf_core_fg12_capture_outputs(directory) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg12_cpu_service() -> *const rsf_cpu_session_service {
    static SERVICE: rsf_cpu_session_service = rsf_cpu_session_service {
        struct_size: size_of::<rsf_cpu_session_service>() as u32,
        version: RSF_CPU_SESSION_SERVICE_VERSION,
        report: Some(rsf_fg12_cpu),
        ping: Some(rsf_native_latency_ping),
        execution: None,
    };
    &SERVICE
}
