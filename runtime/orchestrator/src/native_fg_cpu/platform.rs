// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_present::rsf_d3d11_present_graphics;
use crate::sys::rsf_streamline_graphics;
use crate::native_cpu::rsf_native_cpu_rendered_source_set;
use diagnostics::report_stage;
use std::ffi::c_char;
use std::ptr::null_mut;
use windows::Win32::Graphics::{
    Direct3D12::ID3D12Device,
    Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory4},
};
use windows::core::Interface;

/// The presentation bridge, Streamline host and Anti-Lag leaves the CPU owner calls; the native
/// FG CPU fixture supplies doubles with the same names in the fixture-leaves archive, where it reuses
/// only the Streamline host and Anti-Lag ones.
#[cfg_attr(feature = "fixture-leaves", allow(dead_code, unused_imports))]
pub(super) mod leaf {
    use super::{Handle, rsf_d3d11_present_status, rsf_streamline_latency_status};
    pub(in crate::native_fg_cpu) use super::{adapter_vendor, native_device as device};
    pub(in crate::native_fg_cpu) use crate::d3d11_present::{
        rsf_d3d11_present_abort as abort, rsf_d3d11_present_acquire as acquire,
        rsf_d3d11_present_backend as backend, rsf_d3d11_present_generation as generation,
        rsf_d3d11_present_has_owner as has_owner, rsf_d3d11_present_host as host,
        rsf_d3d11_present_input as input, rsf_d3d11_present_marker as marker,
        rsf_d3d11_present_pace_for_generation as pace, rsf_d3d11_present_receipt as receipt,
    };
    pub(in crate::native_fg_cpu) use crate::latency_antilag::create_sdk_context as create_antilag;
    use crate::streamline_host as host;
    pub(in crate::native_fg_cpu) use rsf_game_sys::rsf_frame_allows_fg as allows_fg;

    /// The fixture's window-thread and completion-wait doubles; production has none.
    pub(in crate::native_fg_cpu) const FIXTURE_OWNED_WINDOW_THREAD: Option<
        unsafe extern "C" fn(u64) -> u32,
    > = None;
    pub(in crate::native_fg_cpu) const FIXTURE_WAIT_COMPLETION: Option<
        unsafe extern "C" fn(u32, u32) -> i32,
    > = None;

    pub(in crate::native_fg_cpu) unsafe fn status(out: *mut rsf_d3d11_present_status) -> i32 {
        unsafe { crate::d3d11_present::rsf_d3d11_present_status_get(out) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_status(
        host: Handle,
        out: *mut rsf_streamline_latency_status,
    ) -> i32 {
        unsafe { host::rsf_streamline_host_latency_status(host.cast(), out) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_reflex(
        host: Handle,
        mode: u32,
        interval: u32,
    ) -> i32 {
        unsafe { host::rsf_streamline_host_reflex(host.cast(), mode, interval) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_token(host: Handle, source: u64) -> Handle {
        unsafe { host::rsf_streamline_host_token(host.cast(), source) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_presented(host: Handle, source: u64) -> i32 {
        unsafe { host::rsf_streamline_host_presented(host.cast(), source) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_abort(host: Handle, source: u64) -> i32 {
        unsafe { host::rsf_streamline_host_abort(host.cast(), source) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_retire(host: Handle, through: u64) -> i32 {
        unsafe { host::rsf_streamline_host_retire_through(host.cast(), through) }
    }
    pub(in crate::native_fg_cpu) unsafe fn host_reset(host: Handle) -> i32 {
        unsafe { host::rsf_streamline_host_reset_sources(host.cast()) }
    }
}
pub(super) unsafe extern "C" fn native_device() -> Handle {
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { rsf_d3d11_present_graphics(&mut graphics) } != 0 {
        graphics.native_device
    } else {
        null_mut()
    }
}

pub(super) unsafe extern "C" fn adapter_vendor(raw: Handle) -> u32 {
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
    unsafe { report_stage(stage, source, frame, result) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_cpu(event: *const Event) {
    unsafe { fg_cpu(event) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_frame(pass: *const Pass, begin: u32) {
    unsafe { fg_frame(pass, begin) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_fg_submission(pass: *const Pass, begin: u32) {
    unsafe { fg_submission(pass, begin) };
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_graphics(api: u32) {
    rsf_core_native_latency_graphics(api);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_render(source: u64, begin: u32) {
    latency_render(source, begin);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_present(source: u64, chain: Handle, begin: u32) {
    observe_latency_present(source, chain, begin);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_status_get(out: *mut LatencyStatus) -> i32 {
    unsafe { latency_status_get(out) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_frame_get(out: *mut LatencyFrame) -> i32 {
    unsafe { rsf_core_native_latency_frame_get(out) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_latency_window_bind(binding: *const Window) -> i32 {
    unsafe { latency_window_bind(binding) }
}

#[cfg(not(test))]
pub(crate) fn bind_latency_window(session_id: u64, window_key: u64) -> i32 {
    bind_window(session_id, window_key)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_input_routes_get(out: *mut Routes) -> i32 {
    unsafe { rsf_core_native_input_routes_get(out) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_ping(window: Handle) -> i32 {
    latency_ping(window)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_retire(session: u64, through: u64) {
    latency_retire(session, through);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_end_session(session: u64) {
    latency_end_session(session);
}
