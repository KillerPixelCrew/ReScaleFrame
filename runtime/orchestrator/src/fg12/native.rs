// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d12_host::fg::*;
use crate::d3d12_host::pass_depth_domains;
use crate::frame_capture12::native::{
    rsf_fg12_platform_map, rsf_fg12_platform_readback, rsf_fg12_platform_readback_copy,
    rsf_fg12_platform_unmap,
};
use crate::native_cpu::rsf_native_cpu_dispatch;
use crate::render_abi::rsf_generation_provider;
use rsf_game_sys::frame_policy::rsf_frame_allows_fg;

unsafe extern "C" {
    fn rsf_input_dump_fg_prepare(
        pass: *const rsf_game_render_pass,
        frame: *const rsf_fg_frame,
        result: i32,
    );
    fn rsf_streamline_host_abort(host: *mut c_void, source: u64) -> i32;
    fn rsf_fg12_platform_install(
        setup: *const rsf_d3d11_present_setup,
        explicit_creation: i32,
    ) -> i32;
    fn rsf_fg12_platform_dump_record(
        list: *mut c_void,
        source: *mut c_void,
        state: u32,
        record: *const rsf_frame_record,
        role: *const c_char,
        slot: u32,
    );
    fn rsf_fg12_platform_dump_flush();
    fn rsf_fg12_platform_dump_submitted(queue: *mut c_void, record: *const rsf_frame_record);
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
    unsafe { rsf_fg12_platform_readback_copy(list, source, state, buffer, &*layout) };
}

pub(super) static CALLS: rsf_fg12_calls = rsf_fg12_calls {
    struct_size: size_of::<rsf_fg12_calls>() as u32,
    buffer_index: Some(rsf_fg12_platform_buffer_index),
    install: Some(rsf_fg12_platform_install),
    allows_fg: Some(allows_fg),
    depth_domains: Some(pass_depth_domains),
    texture_desc: Some(rsf_fg12_platform_texture_desc),
    texture: Some(rsf_fg12_platform_texture),
    copy: Some(rsf_fg12_platform_copy),
    transition: Some(rsf_fg12_platform_transition),
    fence: Some(rsf_fg12_platform_fence),
    signal: Some(rsf_fg12_platform_signal),
    readback: Some(rsf_fg12_platform_readback),
    readback_copy: Some(readback_copy),
    map: Some(rsf_fg12_platform_map),
    unmap: Some(rsf_fg12_platform_unmap),
    dump_record: Some(rsf_fg12_platform_dump_record),
    dump_flush: Some(rsf_fg12_platform_dump_flush),
    dump_submitted: Some(rsf_fg12_platform_dump_submitted),
    dump_prepare: Some(dump_prepare),
    present_queue: Some(rsf_d3d11_present_queue),
    source_queue: Some(rsf_d3d11_present_source_queue),
    present_retire: Some(rsf_d3d11_present_retire),
    is_owner: Some(rsf_d3d11_present_is_owner),
    has_owner: Some(rsf_d3d11_present_has_owner),
    present_provider: Some(rsf_d3d11_present_provider),
    present_backend: Some(rsf_d3d11_present_backend),
    present_generation: Some(rsf_d3d11_present_generation),
    host_abort: Some(host_abort),
    fg_set_log: Some(rsf_native_fg_set_log),
    fg_options_set: Some(rsf_native_fg_options_set),
    fg_cpu: Some(rsf_native_fg_cpu),
    latency_status_get: Some(rsf_native_latency_status_get),
    latency_frame_get: Some(rsf_native_latency_frame_get),
    latency_graphics: Some(rsf_native_latency_graphics),
    latency_render: Some(rsf_native_latency_render),
    latency_present: Some(rsf_native_latency_present),
    latency_end_session: Some(rsf_native_latency_end_session),
    cpu_dispatch: Some(rsf_native_cpu_dispatch),
    dump_poll: Some(rsf_input_dump_poll),
    dump_begin_record: Some(rsf_input_dump_begin_record),
    dump_current: Some(rsf_input_dump_current),
    dump_requested: Some(rsf_input_dump_requested),
    dump_complete: Some(rsf_input_dump_complete),
    dump_result: Some(rsf_input_dump_result),
};
