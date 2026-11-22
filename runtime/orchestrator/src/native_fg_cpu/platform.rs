// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_present::{
    rsf_core_d3d11p_backend, rsf_core_d3d11p_generation, rsf_core_d3d11p_graphics,
    rsf_core_d3d11p_has_owner, rsf_core_d3d11p_host,
};
use crate::d3d11_present_abi::rsf_streamline_graphics;
use crate::fg_session::rsf_core_frame_allows_fg;
use crate::native_cpu::rsf_native_cpu_rendered_source_set;
use diagnostics::rsf_core_native_fg_stage;
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
    fn rsf_d3d11_present_pace_for_generation(source: u64, pacer: u32, generation: u64) -> i32;
    fn rsf_d3d11_present_marker(generation: u64, source: u64, marker: u32, controller: u32) -> i32;
    fn rsf_d3d11_present_input(source: u64, kind: u32, message: u32) -> i32;
    fn rsf_d3d11_present_receipt(generation: u64, source: u64, kind: u32, message: u32) -> i32;
    fn rsf_d3d11_present_abort(source: u64) -> i32;
    fn rsf_native_antilag_context_create() -> Handle;
}

#[unsafe(no_mangle)]
pub(crate) static rsf_native_fg_cpu_platform: Platform = Platform {
    has_owner: Some(rsf_core_d3d11p_has_owner),
    generation: Some(rsf_core_d3d11p_generation),
    backend: Some(rsf_core_d3d11p_backend),
    status: Some(rsf_d3d11_present_status_get),
    device: Some(native_device),
    adapter_vendor: Some(adapter_vendor),
    host: Some(rsf_core_d3d11p_host),
    host_status: Some(rsf_streamline_host_latency_status),
    host_reflex: Some(rsf_streamline_host_reflex),
    host_token: Some(rsf_streamline_host_token),
    host_presented: Some(rsf_streamline_host_presented),
    host_abort: Some(rsf_streamline_host_abort),
    host_retire: Some(rsf_streamline_host_retire_through),
    host_reset: Some(rsf_streamline_host_reset_sources),
    acquire: Some(rsf_d3d11_present_acquire),
    pace: Some(rsf_d3d11_present_pace_for_generation),
    marker: Some(rsf_d3d11_present_marker),
    input: Some(rsf_d3d11_present_input),
    receipt: Some(rsf_d3d11_present_receipt),
    abort: Some(rsf_d3d11_present_abort),
    create_antilag: Some(rsf_native_antilag_context_create),
    fixture_owned_window_thread: None,
    fixture_wait_completion: None,
    allows_fg: Some(rsf_core_frame_allows_fg),
};

unsafe extern "C" fn native_device() -> Handle {
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..rsf_streamline_graphics::EMPTY
    };
    if unsafe { rsf_core_d3d11p_graphics(&mut graphics) } != 0 {
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
extern "C" fn rsf_native_fg_rendered_source_set(enabled: u32) {
    rsf_native_cpu_rendered_source_set(enabled);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_fg_reflex_ordering_set(threaded: u32) {
    rsf_core_native_fg_reflex_ordering_set(threaded);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_stage(
    stage: *const c_char,
    source: u64,
    frame: u64,
    result: i32,
) {
    unsafe { rsf_core_native_fg_stage(stage, source, frame, result, &rsf_native_fg_cpu_platform) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_cpu(event: *const Event) {
    unsafe { rsf_core_native_fg_cpu(event, &rsf_native_fg_cpu_platform) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_frame(pass: *const Pass, begin: u32) {
    unsafe { rsf_core_native_fg_frame(pass, begin, &rsf_native_fg_cpu_platform) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_submission(pass: *const Pass, begin: u32) {
    unsafe { rsf_core_native_fg_submission(pass, begin, &rsf_native_fg_cpu_platform) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_graphics(api: u32) {
    rsf_core_native_latency_graphics(api);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_render(source: u64, begin: u32) {
    rsf_core_native_latency_render(source, begin, &rsf_native_fg_cpu_platform);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_present(source: u64, chain: Handle, begin: u32) {
    rsf_core_native_latency_present(source, chain, begin, &rsf_native_fg_cpu_platform);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_latency_status_get(out: *mut LatencyStatus) -> i32 {
    unsafe { rsf_core_native_latency_status_get(out, &rsf_native_fg_cpu_platform) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_latency_frame_get(out: *mut LatencyFrame) -> i32 {
    unsafe { rsf_core_native_latency_frame_get(out) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_window_bind(binding: *const Window) -> i32 {
    unsafe { rsf_core_native_latency_window_bind(binding, &rsf_native_fg_cpu_platform) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_input_routes_get(out: *mut Routes) -> i32 {
    unsafe { rsf_core_native_input_routes_get(out) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_ping(window: Handle) -> i32 {
    rsf_core_native_latency_ping(window, &rsf_native_fg_cpu_platform)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_retire(session: u64, through: u64) {
    rsf_core_native_latency_retire(session, through, &rsf_native_fg_cpu_platform);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_end_session(session: u64) {
    rsf_core_native_latency_end_session(session, &rsf_native_fg_cpu_platform);
}
