// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d12_host::fg::*;
use crate::d3d12_host::pass_depth_domains;
use crate::frame_capture12::native::{
    rsf_fg12_platform_map, rsf_fg12_platform_readback, rsf_fg12_platform_readback_copy,
    rsf_fg12_platform_unmap,
};
use crate::frame_capture12::{self as capture, Owner as Capture};
use crate::native_cpu::rsf_native_cpu_dispatch;
use rsf_game_sys::rsf_frame_allows_fg;
use std::sync::atomic::AtomicPtr;

unsafe extern "C" {
    fn rsf_input_dump_fg_prepare(
        pass: *const rsf_game_render_pass,
        frame: *const rsf_fg_frame,
        result: i32,
    );
    fn rsf_native_latency_ping(window: *mut c_void) -> i32;
    fn rsf_streamline_host_abort(host: *mut c_void, source: u64) -> i32;
    fn rsf_d3d11_present_install(setup: *const rsf_d3d11_present_setup) -> i32;
    fn rsf_d3d11_present_prepare(setup: *const rsf_d3d11_present_setup) -> i32;
    fn rsf_d3d11_present_queue() -> *mut c_void;
    fn rsf_d3d11_present_source_queue() -> *mut c_void;
    fn rsf_d3d11_present_retire(
        provider_session: *mut c_void,
        list: *mut c_void,
        fence: *mut *mut c_void,
        value: *mut u64,
    ) -> i32;
    fn rsf_d3d11_present_is_owner(chain: *mut c_void) -> i32;
    fn rsf_d3d11_present_has_owner() -> i32;
    fn rsf_d3d11_present_provider() -> *const rsf_generation_provider;
    fn rsf_d3d11_present_backend() -> u32;
    fn rsf_d3d11_present_generation() -> u64;
    fn rsf_native_fg_set_log(log: rsf_backend_log_fn, user: *mut c_void);
    fn rsf_native_fg_options_set(options: *const rsf_native_fg_options);
    fn rsf_native_fg_cpu(event: *const rsf_game_cpu_event);
    fn rsf_native_latency_status_get(status: *mut rsf_native_latency_status) -> i32;
    fn rsf_native_latency_frame_get(frame: *mut rsf_native_latency_frame) -> i32;
    fn rsf_native_latency_graphics(api: u32);
    fn rsf_native_latency_render(source: u64, begin: u32);
    fn rsf_native_latency_present(source: u64, chain: *mut c_void, begin: u32);
    fn rsf_native_latency_end_session(session: u64);
    fn rsf_input_dump_poll();
    fn rsf_input_dump_begin_record(record: *const rsf_frame_record, backend: u32) -> i32;
    fn rsf_input_dump_current(record: *mut rsf_frame_record) -> i32;
    fn rsf_input_dump_requested() -> i32;
    fn rsf_input_dump_complete(record: *const rsf_frame_record, result: i32);
    fn rsf_input_dump_result(
        record: *const rsf_frame_record,
        stage: *const c_char,
        backend: u32,
        result: i32,
    );
}

unsafe extern "C" fn allows_fg(record: *const rsf_frame_record) -> u32 {
    unsafe { rsf_frame_allows_fg(record) as u32 }
}

unsafe extern "C" fn dump_prepare(frame: *const rsf_fg_frame, result: i32) {
    unsafe { rsf_input_dump_fg_prepare(std::ptr::null(), frame, result) };
}

// Explicit creation only prepares the bridge. Both entries are non-unwinding
// Rust behind thin C shims, and their native hook callbacks contain their own
// exceptions, so no foreign unwind can reach this frame.
unsafe extern "C" fn present_install(setup: *const rsf_d3d11_present_setup, explicit: i32) -> i32 {
    unsafe {
        if explicit != 0 {
            rsf_d3d11_present_prepare(setup)
        } else {
            rsf_d3d11_present_install(setup)
        }
    }
}

unsafe extern "C" fn host_abort(host: *mut rsf_streamline_host, source: u64) -> i32 {
    unsafe { rsf_streamline_host_abort(host.cast(), source) }
}

unsafe extern "C" fn readback_copy(
    list: *mut c_void,
    source: *mut c_void,
    state: u32,
    buffer: *mut c_void,
    layout: *const rsf_fg12_readback_layout,
) {
    unsafe { rsf_fg12_platform_readback_copy(list, source, state, buffer, layout) };
}

pub(super) static CALLS: Platform = Platform {
    buffer_index: rsf_fg12_platform_buffer_index,
    install: present_install,
    allows_fg,
    depth_domains: pass_depth_domains,
    texture_desc: rsf_fg12_platform_texture_desc,
    texture: rsf_fg12_platform_texture,
    copy: rsf_fg12_platform_copy,
    transition: rsf_fg12_platform_transition,
    fence: rsf_fg12_platform_fence,
    signal: rsf_fg12_platform_signal,
    readback: rsf_fg12_platform_readback,
    readback_copy,
    map: rsf_fg12_platform_map,
    unmap: rsf_fg12_platform_unmap,
    dump_record,
    dump_flush,
    dump_submitted,
    dump_prepare,
    present_queue: rsf_d3d11_present_queue,
    source_queue: rsf_d3d11_present_source_queue,
    present_retire: rsf_d3d11_present_retire,
    is_owner: rsf_d3d11_present_is_owner,
    has_owner: rsf_d3d11_present_has_owner,
    present_provider: rsf_d3d11_present_provider,
    present_backend: rsf_d3d11_present_backend,
    present_generation: rsf_d3d11_present_generation,
    host_abort,
    fg_set_log: rsf_native_fg_set_log,
    fg_options_set: rsf_native_fg_options_set,
    fg_cpu: rsf_native_fg_cpu,
    latency_status_get: rsf_native_latency_status_get,
    latency_frame_get: rsf_native_latency_frame_get,
    latency_graphics: rsf_native_latency_graphics,
    latency_render: rsf_native_latency_render,
    latency_present: rsf_native_latency_present,
    latency_end_session: rsf_native_latency_end_session,
    cpu_dispatch: rsf_native_cpu_dispatch,
    dump_poll: rsf_input_dump_poll,
    dump_begin_record: rsf_input_dump_begin_record,
    dump_current: rsf_input_dump_current,
    dump_requested: rsf_input_dump_requested,
    dump_complete: rsf_input_dump_complete,
    dump_result: rsf_input_dump_result,
};

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

unsafe extern "C" fn dump_record(
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

unsafe extern "C" fn dump_flush() {
    unsafe { capture::rsf_core_capture12_flush(CAPTURE.load(Ordering::Acquire), u32::MAX, 1) };
}

unsafe extern "C" fn dump_submitted(queue: *mut c_void, record: *const rsf_frame_record) {
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
