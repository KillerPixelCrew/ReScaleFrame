// SPDX-License-Identifier: MIT
//! Typed Windows and Rust owner calls for the host's production platform table.
use crate::d3d11_device_identity::identity;
use crate::d3d11_host_abi::{rsf_d3d11h_pipeline_start, rsf_d3d11h_pipeline_status};
use crate::dlss_pipeline::exports::{
    rsf_dlss_pipeline_get_status, rsf_dlss_pipeline_resize_output,
    rsf_dlss_pipeline_select_backend, rsf_dlss_pipeline_set_colour_correction,
    rsf_dlss_pipeline_set_colour_transport, rsf_dlss_pipeline_set_preset,
    rsf_dlss_pipeline_set_quality, rsf_dlss_pipeline_start_backend,
};
use crate::dlss_streamline::exports::rsf_dlss_native_present;
use crate::native_cpu::{rsf_native_cpu_dispatch, rsf_native_cpu_event};
use crate::native_fg::rsf_native_fg_present;
use crate::native_fg_cpu::platform::{rsf_native_fg_cpu, rsf_native_latency_window_bind};
use crate::native_fg_cpu_abi::rsf_native_latency_window;
use crate::scene_abi::rsf_observer_present_event;
use rsf_game_sys::rsf_game_cpu_event;
use rsf_game_sys::sr_abi::{
    RSF_DLSS_PIPELINE_ABI_VERSION, rsf_dlss_pipeline_setup, rsf_dlss_pipeline_status,
};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIDeviceSubObject, IDXGISwapChain};
use windows::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};
use windows::core::{IUnknown, Interface, PCWSTR};

type Raw = *mut c_void;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_fg_latency_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { rsf_native_fg_present(event) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_latency_window_bind(session_id: u64, window: Raw) {
    let binding = rsf_native_latency_window {
        struct_size: size_of::<rsf_native_latency_window>() as u32,
        version: 1,
        session_id,
        window_key: window.addr() as u64,
    };
    let _ = unsafe { rsf_native_latency_window_bind(&binding) };
}

unsafe extern "C" fn forward_fg(_: Raw, event: *const rsf_game_cpu_event) {
    unsafe { rsf_native_fg_cpu(event) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_cpu_dispatch_fg(event: *const rsf_game_cpu_event) -> i32 {
    unsafe { rsf_native_cpu_dispatch(event, Some(forward_fg), null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_cpu_event(event: *const rsf_game_cpu_event) {
    let _ = unsafe { rsf_native_cpu_event(event) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_native_present(
    chain: Raw,
    sync: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    unsafe { rsf_dlss_native_present(chain, sync, flags, begin) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_start_pipeline(
    device: Raw,
    request: *const rsf_d3d11h_pipeline_start,
) -> i32 {
    let request = unsafe { &*request };
    let setup = unsafe { &*request.setup };
    let text = unsafe { &*request.strings };
    let create = rsf_dlss_pipeline_setup {
        struct_size: size_of::<rsf_dlss_pipeline_setup>() as u32,
        abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
        streamline_directory_utf8: text.streamline,
        streamline_log_directory_utf8: null(),
        require_signature: 1,
        output_width: request.output_width,
        output_height: request.output_height,
        quality: request.quality,
        motion: setup.motion,
        log: request.log,
        log_user: request.user,
        engine: setup.engine_type,
        engine_version_utf8: text.engine_version,
        project_id_utf8: text.project,
        fsr2_directory_utf8: text.fidelityfx,
        fsr3_directory_utf8: text.fidelityfx,
        fsr4_directory_utf8: text.fidelityfx,
        xess_directory_utf8: text.xess,
        view_space_to_meters: setup.units_to_meters,
        standard_depth: u32::from(setup.depth_inverted == 0),
    };
    unsafe { rsf_dlss_pipeline_start_backend(device, &create, request.backend) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_pipeline_resize(width: u32, height: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_resize_output(width, height, null_mut(), null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_pipeline_select(backend: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_select_backend(backend, null_mut(), null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_pipeline_quality(quality: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_set_quality(quality, null_mut(), null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_pipeline_preset(preset: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_set_preset(preset) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_read_pipeline_status(
    output: *mut rsf_d3d11h_pipeline_status,
) -> i32 {
    // The wire record contains only integer fields.
    let mut status = rsf_dlss_pipeline_status {
        struct_size: size_of::<rsf_dlss_pipeline_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let result = unsafe { rsf_dlss_pipeline_get_status(&mut status) };
    unsafe {
        output.write(rsf_d3d11h_pipeline_status {
            running: status.running,
            output_width: status.output_width,
            output_height: status.output_height,
            render_width: status.render_width,
            render_height: status.render_height,
        });
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11h_pipeline_colour(transport: u32, correction: u32) {
    rsf_dlss_pipeline_set_colour_transport(transport);
    rsf_dlss_pipeline_set_colour_correction(correction);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_same_object(left: Raw, right: Raw) -> i32 {
    if right.is_null() {
        return 0;
    }
    let Some(left) = (unsafe { identity(left) }) else {
        return 0;
    };
    i32::from(unsafe { identity(right) }.is_some_and(|right| left.as_raw() == right.as_raw()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_immediate_context(device: Raw) -> Raw {
    unsafe { ID3D11Device::from_raw_borrowed(&device) }
        .and_then(|device| unsafe { device.GetImmediateContext() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn device_vendor(device: Raw) -> Option<u32> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let dxgi = device.cast::<IDXGIDevice>().ok()?;
    let desc = unsafe { dxgi.GetAdapter()?.GetDesc() }.ok()?;
    Some(desc.VendorId)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_device_vendor(device: Raw, vendor: *mut u32) -> i32 {
    let Some(id) = (unsafe { device_vendor(device) }) else {
        return 0;
    };
    unsafe { vendor.write(id) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_device(chain: Raw) -> Raw {
    unsafe { IDXGIDeviceSubObject::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDevice::<ID3D11Device>() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_window(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDesc() }.ok())
        .map_or(null_mut(), |desc| desc.OutputWindow.0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_buffer(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_com_identity(object: Raw) -> Raw {
    unsafe { identity(object) }.map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_present(chain: Raw) -> Raw {
    // Log the entry from this receiver's own vtable, without dispatching it.
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .map_or(null_mut(), |chain| chain.vtable().Present as Raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_com_release(object: Raw) -> u32 {
    let Some(object) = (unsafe { IUnknown::from_raw_borrowed(&object) }) else {
        return 0;
    };
    unsafe { (object.vtable().Release)(object.as_raw()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_ini_read(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    fallback: i32,
) -> i32 {
    unsafe { GetPrivateProfileIntW(PCWSTR(section), PCWSTR(key), fallback, PCWSTR(path)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_ini_write(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    value: *const u16,
) -> i32 {
    i32::from(unsafe {
        WritePrivateProfileStringW(PCWSTR(section), PCWSTR(key), PCWSTR(value), PCWSTR(path))
            .is_ok()
    })
}
