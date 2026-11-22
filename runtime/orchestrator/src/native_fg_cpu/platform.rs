// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_present::{
    rsf_d3d11_present_backend, rsf_d3d11_present_generation, rsf_d3d11_present_graphics,
    rsf_d3d11_present_has_owner, rsf_d3d11_present_host, rsf_d3d11_present_pace_for_generation,
};
use crate::d3d11_present_abi::rsf_streamline_graphics;
use crate::native_cpu::rsf_native_cpu_rendered_source_set;
use diagnostics::report_stage;
use rsf_game_sys::rsf_frame_allows_fg;
use std::ffi::c_char;
use std::ptr::null_mut;
use windows::Win32::Graphics::{
    Direct3D12::ID3D12Device,
    Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory4},
};
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_d3d11_present_status_get(status: *mut rsf_d3d11_present_status) -> i32;
    fn rsf_streamline_host_latency_status(
        host: Handle,
        status: *mut rsf_streamline_latency_status,
    ) -> i32;
    fn rsf_streamline_host_reflex(host: Handle, mode: u32, interval: u32) -> i32;
    fn rsf_streamline_host_token(host: Handle, source: u64) -> Handle;
    fn rsf_streamline_host_presented(host: Handle, source: u64) -> i32;
    fn rsf_streamline_host_abort(host: Handle, source: u64) -> i32;
    fn rsf_streamline_host_retire_through(host: Handle, through: u64) -> i32;
    fn rsf_streamline_host_reset_sources(host: Handle) -> i32;
    fn rsf_d3d11_present_acquire(source: u64) -> i32;
    fn rsf_d3d11_present_marker(generation: u64, source: u64, marker: u32, controller: u32) -> i32;
    fn rsf_d3d11_present_input(source: u64, kind: u32, message: u32) -> i32;
    fn rsf_d3d11_present_receipt(generation: u64, source: u64, kind: u32, message: u32) -> i32;
    fn rsf_d3d11_present_abort(source: u64) -> i32;
}

pub(crate) static NATIVE_PLATFORM: Platform = Platform {
    has_owner: rsf_d3d11_present_has_owner,
    generation: rsf_d3d11_present_generation,
    backend: rsf_d3d11_present_backend,
    status: rsf_d3d11_present_status_get,
    device: native_device,
    adapter_vendor,
    host: rsf_d3d11_present_host,
    host_status: rsf_streamline_host_latency_status,
    host_reflex: rsf_streamline_host_reflex,
    host_token: rsf_streamline_host_token,
    host_presented: rsf_streamline_host_presented,
    host_abort: rsf_streamline_host_abort,
    host_retire: rsf_streamline_host_retire_through,
    host_reset: rsf_streamline_host_reset_sources,
    acquire: rsf_d3d11_present_acquire,
    pace: rsf_d3d11_present_pace_for_generation,
    marker: rsf_d3d11_present_marker,
    input: rsf_d3d11_present_input,
    receipt: rsf_d3d11_present_receipt,
    abort: rsf_d3d11_present_abort,
    create_antilag: crate::latency_antilag::create_sdk_context,
    fixture_owned_window_thread: None,
    fixture_wait_completion: None,
    allows_fg: rsf_frame_allows_fg,
};

#[unsafe(no_mangle)]
pub(crate) static rsf_native_fg_cpu_platform: rsf_nfg_cpu_platform = rsf_nfg_cpu_platform {
    has_owner: Some(NATIVE_PLATFORM.has_owner),
    generation: Some(NATIVE_PLATFORM.generation),
    backend: Some(NATIVE_PLATFORM.backend),
    status: Some(NATIVE_PLATFORM.status),
    device: Some(NATIVE_PLATFORM.device),
    adapter_vendor: Some(NATIVE_PLATFORM.adapter_vendor),
    host: Some(NATIVE_PLATFORM.host),
    host_status: Some(NATIVE_PLATFORM.host_status),
    host_reflex: Some(NATIVE_PLATFORM.host_reflex),
    host_token: Some(NATIVE_PLATFORM.host_token),
    host_presented: Some(NATIVE_PLATFORM.host_presented),
    host_abort: Some(NATIVE_PLATFORM.host_abort),
    host_retire: Some(NATIVE_PLATFORM.host_retire),
    host_reset: Some(NATIVE_PLATFORM.host_reset),
    acquire: Some(NATIVE_PLATFORM.acquire),
    pace: Some(NATIVE_PLATFORM.pace),
    marker: Some(NATIVE_PLATFORM.marker),
    input: Some(NATIVE_PLATFORM.input),
    receipt: Some(NATIVE_PLATFORM.receipt),
    abort: Some(NATIVE_PLATFORM.abort),
    create_antilag: Some(NATIVE_PLATFORM.create_antilag),
    fixture_owned_window_thread: NATIVE_PLATFORM.fixture_owned_window_thread,
    fixture_wait_completion: NATIVE_PLATFORM.fixture_wait_completion,
    allows_fg: Some(NATIVE_PLATFORM.allows_fg),
};

unsafe extern "C" fn native_device() -> Handle {
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..rsf_streamline_graphics::EMPTY
    };
    if unsafe { rsf_d3d11_present_graphics(&mut graphics) } != 0 {
        graphics.native_device
    } else {
        null_mut()
    }
}

unsafe extern "C" fn adapter_vendor(raw: Handle) -> u32 {
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    let desc = unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|adapter| adapter.GetDesc1())
    };
    match desc {
        Ok(desc) if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 => desc.VendorId,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_fg_rendered_source_set(enabled: u32) {
    rsf_native_cpu_rendered_source_set(enabled);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_fg_reflex_ordering_set(threaded: u32) {
    rsf_core_native_fg_reflex_ordering_set(threaded);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_stage(
    stage: *const c_char,
    source: u64,
    frame: u64,
    result: i32,
) {
    unsafe { report_stage(stage, source, frame, result, &NATIVE_PLATFORM) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_cpu(event: *const Event) {
    unsafe { fg_cpu(event, &NATIVE_PLATFORM) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_frame(pass: *const Pass, begin: u32) {
    unsafe { fg_frame(pass, begin, &NATIVE_PLATFORM) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_submission(pass: *const Pass, begin: u32) {
    unsafe { fg_submission(pass, begin, &NATIVE_PLATFORM) };
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_graphics(api: u32) {
    rsf_core_native_latency_graphics(api);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_render(source: u64, begin: u32) {
    latency_render(source, begin, &NATIVE_PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_present(source: u64, chain: Handle, begin: u32) {
    observe_latency_present(source, chain, begin, &NATIVE_PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_status_get(out: *mut LatencyStatus) -> i32 {
    unsafe { latency_status_get(out, &NATIVE_PLATFORM) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_frame_get(out: *mut LatencyFrame) -> i32 {
    unsafe { rsf_core_native_latency_frame_get(out) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_window_bind(binding: *const Window) -> i32 {
    unsafe { latency_window_bind(binding, &NATIVE_PLATFORM) }
}

#[cfg(not(test))]
pub(crate) fn bind_latency_window(session_id: u64, window_key: u64) -> i32 {
    bind_window(session_id, window_key, &NATIVE_PLATFORM)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_input_routes_get(out: *mut Routes) -> i32 {
    unsafe { rsf_core_native_input_routes_get(out) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_ping(window: Handle) -> i32 {
    latency_ping(window, &NATIVE_PLATFORM)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_retire(session: u64, through: u64) {
    latency_retire(session, through, &NATIVE_PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_end_session(session: u64) {
    latency_end_session(session, &NATIVE_PLATFORM);
}
