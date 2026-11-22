// SPDX-License-Identifier: MIT

#[cfg(windows)]
mod auxiliary_sr;
#[cfg(windows)]
mod auxiliary_sr_abi;
#[cfg(windows)]
mod callback_host;
mod capture_metadata;
mod capture_names;
mod capture_pixels;
#[cfg(test)]
mod capture_test_sink;
#[cfg(windows)]
mod colour_d3d11;
#[cfg(windows)]
mod com_ref;
#[cfg(windows)]
mod constant_buffer_read;
#[cfg(windows)]
mod constant_twins;
#[cfg(windows)]
mod cpu_abi;
#[cfg(windows)]
mod cpu_reservations;
#[cfg(windows)]
mod d3d11_d3d12;
#[cfg(windows)]
mod d3d11_device_identity;
#[cfg(windows)]
mod d3d11_host;
#[cfg(windows)]
mod d3d11_host_abi;
#[cfg(windows)]
mod d3d11_host_platform;
#[cfg(windows)]
mod d3d11_host_reduced;
#[cfg(windows)]
mod d3d11_observer_abi;
#[cfg(windows)]
mod d3d11_observer_callbacks;
#[cfg(windows)]
mod d3d11_observer_capture;
#[cfg(windows)]
mod d3d11_observer_hooks;
#[cfg(windows)]
#[path = "../../graphics/src/d3d11_observer_layout_abi.rs"]
mod d3d11_observer_layout_abi;
#[cfg(windows)]
mod d3d11_observer_module;
#[cfg(windows)]
mod d3d11_observer_present;
#[cfg(windows)]
mod d3d11_output;
#[cfg(windows)]
mod d3d11_output_abi;
#[cfg(windows)]
mod d3d11_output_platform;
#[cfg(windows)]
mod d3d11_present;
#[cfg(windows)]
mod d3d11_present_abi;
#[cfg(windows)]
mod d3d11_sharpening;
#[cfg(windows)]
mod d3d11_sr_admission;
#[cfg(all(windows, not(test)))]
mod d3d11_sr_api;
#[cfg(windows)]
mod d3d11_state;
#[cfg(windows)]
mod d3d12_fence;
#[cfg(windows)]
mod d3d12_generation;
#[cfg(windows)]
mod d3d12_host;
#[cfg(windows)]
mod d3d12_observer;
#[cfg(windows)]
mod d3d12_observer_abi;
#[cfg(windows)]
mod d3d12_sharpening;
#[cfg(windows)]
mod d3d12_sr;
#[cfg(any(windows, test))]
mod d3d12_window_presenter;
#[cfg(windows)]
mod depth_replay;
#[cfg(windows)]
mod depth_scale;
#[cfg(windows)]
mod diagnostic_output;
#[cfg(windows)]
mod dlss_native12;
#[cfg(windows)]
mod dlss_pipeline;
#[cfg(windows)]
mod dlss_pipeline_abi;
#[cfg(windows)]
mod dlss_streamline;
#[cfg(windows)]
mod dlss_streamline_abi;
#[cfg(windows)]
mod dlss_vulkan_capture;
#[cfg(windows)]
mod dlss_vulkan_capture_ledger;
#[cfg(windows)]
mod dx12_vulkan_interop;
#[cfg(windows)]
mod dx12_vulkan_interop_abi;
#[cfg(windows)]
mod dx12_vulkan_platform;
#[cfg(windows)]
#[path = "../../graphics/src/dxbc_chunks.rs"]
mod dxbc_chunks;
#[cfg(windows)]
mod dxgi_format;
#[cfg(windows)]
mod graphics_d3d11;
#[cfg(windows)]
#[path = "../../../tests/graphics_fixture.rs"]
mod graphics_fixture;
#[cfg(windows)]
mod graphics_pass_abi;
#[cfg(windows)]
mod mfg_state;
#[cfg(windows)]
mod motion_decode;
#[cfg(windows)]
mod motion_resolve;
#[cfg(windows)]
mod motion_resolve_params;
#[cfg(windows)]
mod native_composition;
#[cfg(windows)]
mod native_cpu;
#[cfg(windows)]
mod native_exposure;
#[cfg(windows)]
mod native_fg;
#[cfg(windows)]
mod native_fg_abi;
#[cfg(windows)]
mod native_fg_cpu;
#[cfg(windows)]
mod native_fg_cpu_abi;
#[cfg(windows)]
mod native_regions;
#[cfg(windows)]
mod native_reinsert;
#[cfg(windows)]
mod native_translucency;
mod owned_handle;

#[cfg(windows)]
mod d3d11_fg_api_abi;
#[cfg(windows)]
mod ffx_version;
#[cfg(windows)]
mod fg12;
#[cfg(windows)]
mod fg12_abi;
#[cfg(windows)]
mod fg_choice;
#[cfg(windows)]
mod fg_dispatch;
#[cfg(windows)]
mod fg_leases;
#[cfg(windows)]
#[path = "../../../ui/overlay/src/fg_reasons.rs"]
mod fg_reasons;
#[cfg(windows)]
mod fg_scene;
#[cfg(windows)]
mod fg_session;
#[cfg(windows)]
mod fg_status;
#[cfg(windows)]
mod frame_assembly;
#[cfg(windows)]
mod frame_capture12;
mod frame_capture_vk;
#[cfg(windows)]
mod frame_limit;
mod frame_sequencer;
#[cfg(windows)]
mod frame_tap;
#[cfg(windows)]
mod frame_tap_abi;
#[cfg(windows)]
mod fsr4_fg_bridge;
#[cfg(windows)]
mod fullscreen_pass;
#[cfg(windows)]
mod gl_pipeline;
#[cfg(windows)]
mod gl_pipeline_abi;
#[cfg(windows)]
mod gl_shared_surface;
#[cfg(windows)]
mod input_dump;
#[cfg(windows)]
mod input_dump_vulkan;
#[cfg(windows)]
mod input_receipts;
#[cfg(windows)]
mod latency_antilag;
#[cfg(windows)]
mod latency_lease;
#[cfg(windows)]
mod native_scene;
#[cfg(windows)]
mod native_sr;
#[cfg(windows)]
mod native_sr_abi;
#[cfg(windows)]
mod native_sr_state;
#[cfg(windows)]
mod native_translucency_abi;
#[cfg(windows)]
mod native_window;
#[cfg(windows)]
mod overlay_abi;
#[cfg(windows)]
mod overlay_cursor_capture;
#[cfg(windows)]
mod overlay_cursor_hooks;
#[cfg(windows)]
mod overlay_geometry;
#[cfg(windows)]
mod overlay_host;
#[cfg(windows)]
mod overlay_host_abi;
#[cfg(windows)]
mod overlay_input;
#[cfg(windows)]
mod overlay_input_route;
#[cfg(windows)]
mod overlay_native12;
#[cfg(windows)]
mod overlay_native12_abi;
#[cfg(windows)]
mod overlay_panel;
#[cfg(windows)]
mod overlay_pointer;
#[cfg(windows)]
mod overlay_renderer11;
#[cfg(windows)]
mod overlay_service;
#[cfg(windows)]
mod overlay_settings;
#[cfg(windows)]
mod overlay_visibility;
#[cfg(windows)]
mod overlay_window;
#[cfg(windows)]
mod overlay_wrapped12;
#[cfg(windows)]
mod plugin_session;
#[cfg(windows)]
mod present_rates;
#[cfg(windows)]
mod reflex_audit;
#[cfg(windows)]
mod render_abi;
#[cfg(windows)]
mod render_host;
#[cfg(windows)]
mod render_links;
#[cfg(windows)]
mod resource_roles;
#[cfg(windows)]
mod resource_roles_abi;
#[cfg(windows)]
mod sampler_bias;
#[cfg(windows)]
mod scene_abi;
#[cfg(windows)]
mod scene_promote;
#[cfg(windows)]
mod scene_promote_abi;
#[cfg(windows)]
mod sdk_module;
#[cfg(windows)]
mod settings_store;
#[cfg(windows)]
#[path = "../../graphics/src/shader_hash.rs"]
mod shader_hash;
#[cfg(windows)]
mod shader_patch;
#[cfg(windows)]
mod shader_patch_abi;
#[cfg(windows)]
mod shared_surface;
#[cfg(windows)]
mod sharpening_policy;
#[cfg(windows)]
mod sr12;
#[cfg(windows)]
mod sr_bridge;
mod sr_history;
#[cfg(windows)]
mod sr_legacy;
#[cfg(windows)]
mod sr_session;
#[cfg(windows)]
mod streamline_generation_records;
#[cfg(windows)]
mod streamline_host;
#[cfg(windows)]
mod streamline_log;
#[cfg(windows)]
mod streamline_policy;
#[cfg(windows)]
mod streamline_sr;
#[cfg(all(windows, test))]
mod temporary_ini;
#[cfg(windows)]
mod texture_dump;
#[cfg(windows)]
mod ui_identify;
#[cfg(windows)]
mod ui_identify_abi;
#[cfg(windows)]
mod ui_layer;
#[cfg(windows)]
mod ui_layer_abi;
#[cfg(windows)]
mod vulkan_antilag;
#[cfg(windows)]
mod vulkan_capture_commands;
#[cfg(windows)]
mod vulkan_dispatch;
#[cfg(windows)]
mod vulkan_dlss_fg;
#[cfg(windows)]
mod vulkan_dlss_fg_abi;
#[cfg(windows)]
mod vulkan_dlss_sr;
#[cfg(windows)]
mod vulkan_dlss_sr_abi;
#[cfg(windows)]
mod vulkan_dx12_fg;
#[cfg(windows)]
mod vulkan_external_copy;
#[cfg(windows)]
mod vulkan_ffx_fg;
#[cfg(windows)]
mod vulkan_fg;
#[cfg(windows)]
mod vulkan_fg_abi;
#[cfg(windows)]
mod vulkan_fg_copy;
#[cfg(windows)]
mod vulkan_fg_frames;
#[cfg(windows)]
mod vulkan_fg_resources;
#[cfg(windows)]
mod vulkan_fg_resources_abi;
#[cfg(windows)]
mod vulkan_fg_wsi;
#[cfg(windows)]
mod vulkan_fsr4_fg;
#[cfg(windows)]
mod vulkan_fsr4_sr;
#[cfg(windows)]
mod vulkan_latency;
#[cfg(windows)]
mod vulkan_latency_abi;
#[cfg(windows)]
mod vulkan_latency_frames;
mod vulkan_latency_platform_abi;
#[cfg(windows)]
mod vulkan_ownership;
#[cfg(windows)]
mod vulkan_pcl;
#[cfg(windows)]
mod vulkan_presentation;
#[cfg(windows)]
mod vulkan_sharpening;
#[cfg(windows)]
mod vulkan_sharpening_abi;
#[cfg(windows)]
mod vulkan_sr;
#[cfg(windows)]
mod vulkan_sr_abi;
#[cfg(windows)]
mod vulkan_sr_session;
#[cfg(windows)]
mod vulkan_sr_split;
#[cfg(windows)]
mod vulkan_timing;
#[cfg(windows)]
mod vulkan_xess_fg;
#[cfg(windows)]
mod win32_diagnostics;
#[cfg(windows)]
#[path = "../../vulkan/xess-fg/session.rs"]
mod xess_fg_bridge;

#[unsafe(no_mangle)]
pub extern "C" fn rsf_get_runtime_version() -> *const std::ffi::c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

// C callers can recover from allocation failure. A successful handle is
// released with Box::from_raw after its owner has retired outstanding users.
fn allocate_handle<T>(value: T) -> *mut T {
    const { assert!(size_of::<T>() != 0) }
    let raw = unsafe { std::alloc::alloc(std::alloc::Layout::new::<T>()).cast::<T>() };
    if !raw.is_null() {
        unsafe { raw.write(value) };
    }
    raw
}

#[cfg(windows)]
fn reserve_handle<T>() -> Option<Box<std::mem::MaybeUninit<T>>> {
    let storage = allocate_handle(std::mem::MaybeUninit::<T>::uninit());
    (!storage.is_null()).then(|| unsafe { Box::from_raw(storage) })
}

// Adopt a returned Win32 handle. APIs use both null and -1 to report failure.
#[cfg(windows)]
unsafe fn owned_win32_handle(
    raw: std::os::windows::io::RawHandle,
) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return None;
    }
    Some(unsafe { OwnedHandle::from_raw_handle(raw) })
}

// Public backend IDs from rescaleframe/sr_session.h, checked by the native ABI test.
const NONE: u32 = 0;
const DLSS: u32 = 1;
const FSR3: u32 = 3;
const XESS: u32 = 5;

#[unsafe(no_mangle)]
pub extern "C" fn rsf_gpu_default_sr_backend(vendor: u32, software: u32) -> u32 {
    if software != 0 {
        return NONE;
    }
    match vendor {
        0x10de => DLSS,
        0x8086 => XESS,
        _ => FSR3,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_gpu_select_sr_backend(vendor: u32, software: u32, choices: u32) -> u32 {
    let preferred = rsf_gpu_default_sr_backend(vendor, software);
    if preferred == NONE {
        return NONE;
    }
    [preferred, FSR3, XESS]
        .into_iter()
        .find(|backend| choices & (1 << backend) != 0)
        .unwrap_or(NONE)
}
