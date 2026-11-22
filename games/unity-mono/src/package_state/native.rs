// SPDX-License-Identifier: MIT
//! Package entry points backed by the linked Unity SDK and Runtime shims.
use super::services::backend_log;
use super::{BACKEND_OK, PACKAGE, routing, services};
use crate::bridge_native::package::graphics;
use crate::bridge_present_abi::rsf_d3d11_present_host;
use crate::generation::RSF_DLSS_ENGINE_UNITY;
use crate::package_abi::{
    RSF_DLSS_ABI_VERSION, RSF_SR12_PLAN_ABI_VERSION, rsf_dlss_setup, rsf_sr12,
    rsf_sr12_plan_result, rsf_sr12_setup, rsf_unity_package_view_setup,
};
use crate::preview_abi::rsf_d3d12_window_observer;
use std::ffi::{c_int, c_void};
use std::sync::atomic::Ordering::SeqCst;
use windows::Win32::Graphics::{
    Direct3D12::{ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource},
    Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory4},
};
use windows::core::Interface;

unsafe extern "C" {
    // Runtime entries linked from rsf_orchestrator_core. They are Rust and cannot unwind.
    fn rsf_fg_default_backend(vendor: u32, software: u32) -> u32;
    fn rsf_sr12_create_planned(
        setup: *const rsf_sr12_setup,
        plan: *mut rsf_sr12_plan_result,
        output: *mut *mut rsf_sr12,
    ) -> i32;
    fn rsf_unity_preview_prepare(
        device: *mut c_void,
        queue: *mut c_void,
        width: u32,
        height: u32,
        observer: rsf_d3d12_window_observer,
    ) -> u32;
    fn rsf_unity_preview_region(
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        visible: bool,
    ) -> bool;
    fn rsf_unity_preview_hide();
}

/// Runs inside the bridge's admitted SR callback and borrows the recording list.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_create_view(
    commands: *mut c_void,
    view: *mut rsf_unity_package_view_setup,
    result: *mut i32,
    output: *mut *mut rsf_sr12,
) -> u32 {
    let Some(commands) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&commands) })
    else {
        return 0;
    };
    let mut device = None::<ID3D12Device>;
    if unsafe { commands.GetDevice(&mut device) }.is_err() {
        return 0;
    }
    let Some(device) = device else {
        return 0;
    };
    let view = unsafe { &mut *view };
    let setup = rsf_sr12_setup {
        struct_size: size_of::<rsf_sr12_setup>() as u32,
        abi_version: 1,
        device: device.as_raw(),
        backend: view.backend,
        quality: view.quality,
        output_width: view.output_width,
        output_height: view.output_height,
        render_width: view.render_width,
        render_height: view.render_height,
        inverted_depth: view.inverted_depth,
        streamline_host: unsafe { rsf_d3d11_present_host() }.cast(),
        fsr2_directory_utf8: view.fsr_utf8,
        fsr3_directory_utf8: view.fsr_utf8,
        fsr4_directory_utf8: view.fsr_utf8,
        xess_directory_utf8: view.xess_utf8,
        log: Some(backend_log),
        dlss: rsf_dlss_setup {
            struct_size: size_of::<rsf_dlss_setup>() as u32,
            abi_version: RSF_DLSS_ABI_VERSION,
            interposer_path_utf8: view.interposer_utf8,
            plugin_directory_utf8: view.streamline_utf8,
            log_directory_utf8: view.root_utf8,
            engine: RSF_DLSS_ENGINE_UNITY,
            engine_version_utf8: view.engine_version_utf8,
            project_id_utf8: view.project_id_utf8,
            require_signature: 1,
            log: Some(backend_log),
            preset: view.dlss_preset,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut plan = rsf_sr12_plan_result {
        struct_size: size_of::<rsf_sr12_plan_result>() as u32,
        abi_version: RSF_SR12_PLAN_ABI_VERSION,
        ..Default::default()
    };
    let created = unsafe { rsf_sr12_create_planned(&setup, &mut plan, output) };
    unsafe {
        result.write(if plan.requested_result != BACKEND_OK {
            plan.requested_result
        } else {
            created
        });
    }
    if created == BACKEND_OK {
        view.backend = plan.backend;
        view.render_width = plan.render_width;
        view.render_height = plan.render_height;
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_default_fg(backend: *mut u32) -> u32 {
    let Some((device, _queue)) = graphics() else {
        return 0;
    };
    let desc = unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|adapter| adapter.GetDesc1())
    };
    let Ok(desc) = desc else {
        return 0;
    };
    let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
    unsafe { backend.write(rsf_fg_default_backend(desc.VendorId, u32::from(software))) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_texture_desc(
    resource: *mut c_void,
    width: *mut u64,
    height: *mut u32,
    format: *mut u32,
) {
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return;
    };
    let desc = unsafe { resource.GetDesc() };
    unsafe {
        width.write(desc.Width);
        height.write(desc.Height);
        format.write(desc.Format.0 as u32);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_prepare_preview(width: u32, height: u32) -> u32 {
    let Some((device, queue)) = graphics() else {
        return 0;
    };
    unsafe {
        rsf_unity_preview_prepare(
            device.as_raw(),
            queue.as_raw(),
            width,
            height,
            Some(routing::observe_preview),
        )
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_preview_region(
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    visible: u32,
) -> c_int {
    let configured = unsafe { rsf_unity_preview_region(left, top, width, height, visible != 0) };
    services::rsf_unity_package_preview_input();
    c_int::from(configured)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_shutdown() -> u32 {
    if PACKAGE.services.editor.load(SeqCst) {
        unsafe { rsf_unity_preview_hide() };
    }
    u32::from(routing::shutdown())
}
