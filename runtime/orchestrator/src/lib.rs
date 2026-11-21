// SPDX-License-Identifier: MIT

#[cfg(windows)]
mod auxiliary_sr;
#[cfg(windows)]
mod auxiliary_sr_abi;
#[cfg(windows)]
mod callback_host;
#[cfg(windows)]
mod com_ref;
#[cfg(windows)]
mod cpu_abi;
#[cfg(windows)]
mod cpu_reservations;
#[cfg(windows)]
mod d3d11_host;
#[cfg(windows)]
mod d3d11_host_abi;
#[cfg(windows)]
mod d3d11_host_reduced;
#[cfg(windows)]
mod d3d11_output;
#[cfg(windows)]
mod d3d11_output_abi;
#[cfg(windows)]
mod d3d11_present;
#[cfg(windows)]
mod d3d11_present_abi;
#[cfg(windows)]
mod dlss_pipeline;
#[cfg(windows)]
mod dlss_pipeline_abi;
#[cfg(windows)]
mod dlss_vulkan_capture;
#[cfg(windows)]
mod dlss_vulkan_capture_ledger;
#[cfg(windows)]
mod dx12_vulkan_interop;
#[cfg(windows)]
mod dx12_vulkan_interop_abi;
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
mod fg_scene;
#[cfg(windows)]
mod fg_session;
#[cfg(windows)]
mod fg_status;
#[cfg(windows)]
mod frame_assembly;
mod frame_capture_vk;
mod frame_sequencer;
#[cfg(windows)]
mod gl_pipeline;
#[cfg(windows)]
mod gl_pipeline_abi;
#[cfg(windows)]
mod input_dump;
#[cfg(windows)]
mod input_dump_vulkan;
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
mod native_window;
#[cfg(windows)]
mod overlay_abi;
#[cfg(windows)]
mod overlay_native12;
#[cfg(windows)]
mod overlay_native12_abi;
#[cfg(windows)]
mod overlay_service;
#[cfg(windows)]
mod overlay_wrapped12;
#[cfg(windows)]
mod plugin_session;
#[cfg(windows)]
mod render_abi;
#[cfg(windows)]
mod render_host;
#[cfg(windows)]
mod render_links;
#[cfg(windows)]
mod sampler_bias;
#[cfg(windows)]
mod scene_abi;
#[cfg(windows)]
mod settings_store;
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
mod vulkan_antilag;
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
mod vulkan_ffx_fg;
#[cfg(windows)]
mod vulkan_fg;
#[cfg(windows)]
mod vulkan_fg_abi;
#[cfg(windows)]
mod vulkan_fg_frames;
#[cfg(windows)]
mod vulkan_fg_resources;
#[cfg(windows)]
mod vulkan_fg_resources_abi;
#[cfg(windows)]
mod vulkan_fsr4_fg;
#[cfg(windows)]
mod vulkan_fsr4_sr;
#[cfg(windows)]
mod vulkan_fsr4_sr_abi;
#[cfg(windows)]
mod vulkan_latency;
#[cfg(windows)]
mod vulkan_latency_abi;
#[cfg(windows)]
mod vulkan_latency_frames;
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
mod vulkan_sr_split;
#[cfg(windows)]
mod vulkan_xess_fg;

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
