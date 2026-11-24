// SPDX-License-Identifier: MIT
//! `api.h` records, mirrored by hand (runtime_platform.c asserts the layouts the engine module reads),
//! and the Rust-only records `Platform` passes between this crate's owners and the Runtime, DXGI and
//! Win32. `Platform` is implemented by `host.rs` (D3D12, window, counters) and `vulkan/native.rs`
//! (the Vulkan owner's guard, device, presenter, SDK and SR context calls); every decision stays in
//! this crate. The Vulkan creation leaves are a separate table (`VkCreationLeaves`). The render pass and CPU event are the generated
//! `rsf_game_render_pass` and `rsf_game_cpu_event` bindings.
use rsf_game_sys::{rsf_game_cpu_event, rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::{c_char, c_int, c_void};

/// `RSF_GODOT_ABI_VERSION`: setup, status, engine services and the Vulkan packet.
pub(crate) const ABI_VERSION: u32 = 3;
/// `RSF_GODOT_SETTINGS_ABI_VERSION`.
pub(crate) const SETTINGS_ABI_VERSION: u32 = 2;

pub(crate) use rsf_game_sys::rsf_sharpening_options as Sharpening;

/// `rsf_godot_settings`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Settings {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) sr_backend: u32,
    pub(crate) quality: u32,
    pub(crate) dlss_preset: u32,
    pub(crate) fg_backend: u32,
    pub(crate) fg_mode: u32,
    pub(crate) generated_frames: u32,
    pub(crate) latency_mode: u32,
    pub(crate) frame_limit_us: u32,
    pub(crate) gameplay: u32,
    pub(crate) sharpening: Sharpening,
}

impl Settings {
    pub(crate) fn reconstruction_key(&self) -> (u32, u32, u32) {
        (self.sr_backend, self.quality, self.dlss_preset)
    }
}

/// `rsf_godot_status`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Status {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) session_id: u64,
    pub(crate) requested_sr: u32,
    pub(crate) active_sr: u32,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) sr_result: i32,
    pub(crate) fg_result: i32,
    pub(crate) fg_backend: u32,
    pub(crate) fg_available: u32,
    pub(crate) fg_active: u32,
    pub(crate) max_generated_frames: u32,
    pub(crate) generated_frames: u32,
    pub(crate) latency_available: u32,
    pub(crate) latency_mode: u32,
    pub(crate) reason: u32,
    pub(crate) pcl_available: u32,
    pub(crate) latency_report_available: u32,
    pub(crate) evaluated_frames: u64,
    pub(crate) refused_frames: u64,
    pub(crate) tagged_frames: u64,
    pub(crate) sleep_calls: u64,
    pub(crate) marker_calls: u64,
    pub(crate) total_presented: u64,
    pub(crate) generated_presented: u64,
    pub(crate) presentation_statistics_valid: u32,
    pub(crate) presentation_statistics_flags: u32,
    pub(crate) source_frame_id: u64,
    pub(crate) provider_generation: u64,
    pub(crate) generated_callbacks: u64,
    pub(crate) window_foreground: u32,
    pub(crate) window_visible: u32,
    pub(crate) foreground_process_id: u32,
    pub(crate) keyboard_inputs: u64,
    pub(crate) mouse_inputs: u64,
    pub(crate) controller_inputs: u64,
    pub(crate) pcl_pings: u64,
    pub(crate) latency_owner: u32,
    pub(crate) input_substitutes: u64,
    pub(crate) order_violations: u64,
    pub(crate) derived_boundaries: u64,
}

/// `rsf_godot_setup`.
#[repr(C)]
pub(crate) struct Setup {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) runtime_directory: *const c_char,
    pub(crate) engine_version: *const c_char,
    pub(crate) project_id: *const c_char,
    pub(crate) log: rsf_game_log_fn,
    pub(crate) log_user: *mut c_void,
}

pub(crate) type ConfigureFn = unsafe extern "C" fn(*const Settings, u64) -> c_int;
pub(crate) type ResetFn = unsafe extern "C" fn();

/// `rsf_godot_engine_services`, supplied by the engine module.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct EngineServices {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) configure: Option<ConfigureFn>,
    pub(crate) reset_history: Option<ResetFn>,
}

/// `rsf_godot_fg_options`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FgOptions {
    pub(crate) mode: u32,
    pub(crate) generated_frames: u32,
    pub(crate) reflex_mode: u32,
    pub(crate) frame_limit_us: u32,
    pub(crate) debug: u32,
}

/// `rsf_godot_fg_install`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct FgInstall {
    pub(crate) backend: u32,
    pub(crate) max_generated_frames: u32,
    pub(crate) runtime_switching: u32,
    pub(crate) ui_mode: u32,
    pub(crate) options: FgOptions,
    pub(crate) depth_inverted: u32,
    pub(crate) depth_infinite: u32,
    pub(crate) units_to_meters: f32,
    pub(crate) engine_type: u32,
    pub(crate) fidelityfx_directory: *const c_char,
    pub(crate) xess_directory: *const c_char,
    pub(crate) streamline_directory: *const c_char,
    pub(crate) engine_version: *const c_char,
    pub(crate) project_id: *const c_char,
}

/// `rsf_godot_paths`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Paths {
    pub(crate) runtime_directory: *const c_char,
    pub(crate) streamline_directory: *const c_char,
    pub(crate) streamline_interposer: *const c_char,
    pub(crate) fidelityfx_directory: *const c_char,
    pub(crate) xess_directory: *const c_char,
    pub(crate) engine_version: *const c_char,
    pub(crate) project_id: *const c_char,
}

/// `rsf_godot_input_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct InputFacts {
    pub(crate) keyboard: u64,
    pub(crate) mouse: u64,
    pub(crate) controller: u64,
    pub(crate) pings: u64,
    pub(crate) substitutes: u64,
    pub(crate) order_violations: u64,
    pub(crate) derived_boundaries: u64,
}

/// `rsf_godot_window_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct WindowFacts {
    pub(crate) foreground_process_id: u32,
    pub(crate) current_process_id: u32,
    pub(crate) input_window_visible: u32,
}

/// `rsf_godot_window_identity`: the owner process and class of a candidate window, as read.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowIdentity {
    /// `GetWindowThreadProcessId`; zero when the window is not valid.
    pub(crate) process_id: u32,
    pub(crate) current_process_id: u32,
    /// `GetClassNameW` into `class_name`; zero on failure.
    pub(crate) class_length: u32,
    pub(crate) class_name: [u16; 64],
}

impl Default for WindowIdentity {
    fn default() -> Self {
        Self {
            process_id: 0,
            current_process_id: 0,
            class_length: 0,
            class_name: [0; 64],
        }
    }
}

/// `rsf_godot_fg_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FgFacts {
    pub(crate) available: u32,
    pub(crate) active: u32,
    pub(crate) reason: u32,
    pub(crate) max_generated_frames: u32,
    pub(crate) generated_frames: u32,
    pub(crate) statistics_flags: u32,
    pub(crate) last_result: i32,
    pub(crate) switch_result: i32,
    pub(crate) backend: u32,
    pub(crate) tagged_frames: u64,
    pub(crate) total_presented: u64,
    pub(crate) generated_presented: u64,
    pub(crate) source_frame_id: u64,
    pub(crate) generated_callbacks: u64,
    pub(crate) provider_generation: u64,
}

/// `rsf_godot_latency_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LatencyFacts {
    pub(crate) available: u32,
    pub(crate) effective_mode: u32,
    pub(crate) pcl_available: u32,
    pub(crate) report_available: u32,
    pub(crate) owner: u32,
    pub(crate) paced_frames: u64,
    pub(crate) marker_calls: u64,
    pub(crate) inputs: InputFacts,
}

/// One evaluated D3D12 SR pass as `d3d12.rs` reports it to the status record.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SrFrame {
    pub(crate) requested_sr: u32,
    pub(crate) context_backend: u32,
    pub(crate) context_result: i32,
    pub(crate) evaluated: i32,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
}

/// `rsf_godot_sr_create`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SrCreate {
    pub(crate) backend: u32,
    pub(crate) quality: u32,
    pub(crate) dlss_preset: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) paths: Paths,
}

/// `rsf_godot_sr_context`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SrContext {
    pub(crate) handle: *mut c_void,
    pub(crate) result: i32,
    pub(crate) desired_width: u32,
    pub(crate) desired_height: u32,
}

/// `rsf_godot_sr_evaluate`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SrEvaluate {
    pub(crate) slot: u32,
    pub(crate) screen: u32,
    pub(crate) sharpening: Sharpening,
}

/// `rsf_godot_vk_facts`, read under the Vulkan owner's guard.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkFacts {
    pub(crate) graph_handoff: u32,
    pub(crate) fsr4_sr_compiled: u32,
    pub(crate) fsr4_fg_compiled: u32,
    pub(crate) xess_fg_compiled: u32,
    pub(crate) presenter: u32,
    /// `rsf_slvk_initialize` with DLSS SR and FG succeeded at this session's start.
    pub(crate) streamline_ready: u32,
    pub(crate) presenter_backend: u32,
    pub(crate) sdk_shutdown_pending: u32,
}

/// `rsf_godot_vk_latency_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkLatencyFacts {
    pub(crate) low_latency_available: u32,
    pub(crate) report_available: u32,
    pub(crate) pcl_message: u32,
    pub(crate) mode: u32,
    pub(crate) owner: u32,
    pub(crate) sleep_calls: u64,
    pub(crate) marker_calls: u64,
}

/// `rsf_godot_vk_view`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkView {
    pub(crate) settings: Settings,
    pub(crate) sr_backend: u32,
    pub(crate) generation: u64,
    pub(crate) session_id: u64,
    pub(crate) fg_backend: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
}

/// `rsf_godot_vk_present_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkPresentFacts {
    pub(crate) enabled: u32,
    pub(crate) result: i32,
    pub(crate) generated_callbacks: u64,
    pub(crate) presented: u64,
    pub(crate) tagged: u64,
}

/// One SR pass of `rsfg_render_vulkan` as `vulkan/sr.rs` reports it to the status record.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkSrFrame {
    pub(crate) identity: u32,
    pub(crate) outcome: u32,
    pub(crate) result: i32,
    pub(crate) backend: u32,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) source_frame_id: u64,
}

/// `rsf_godot_vk_sr_pass`. `call` is `vulkan/native.rs`'s private pass record, lent back to the leaves.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkSrPass {
    pub(crate) call: *mut c_void,
    pub(crate) commands: *mut c_void,
    pub(crate) device: *mut c_void,
    pub(crate) device_proc: Option<VkProc>,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) source_frame_id: u64,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
}

/// `rsf_godot_vk_sr_create`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkSrCreate {
    pub(crate) backend: u32,
    pub(crate) quality: u32,
    pub(crate) preset: u32,
}

/// `rsf_godot_vk_sr_context`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VkSrContext {
    pub(crate) handle: *mut c_void,
    pub(crate) result: i32,
}

/// `RSF_GODOT_VK_SCOPE_*`.
pub(crate) const VK_SCOPE_INSTANCE: u32 = 0;
pub(crate) const VK_SCOPE_DEVICE: u32 = 1;
/// `RSF_GODOT_VK_ROUTE_*`.
pub(crate) const VK_ROUTE_NONE: u32 = 0;
pub(crate) const VK_ROUTE_FORWARD: u32 = 1;
pub(crate) const VK_ROUTE_CREATE_INSTANCE: u32 = 2;
pub(crate) const VK_ROUTE_DEVICE_PROC: u32 = 3;
pub(crate) const VK_ROUTE_CREATE_DEVICE: u32 = 4;
pub(crate) const VK_ROUTE_DESTROY_DEVICE: u32 = 5;
pub(crate) mod vulkan_negotiation;
pub(crate) use vulkan_negotiation::{
    VkAdapter, VkChainFacts, VkCreationLeaves, VkDeviceCall, VkDeviceDerive,
    VkFunction, VkInstanceCall, VkLatencyRequirements, VkProc,
};

/// `RSF_GODOT_EFFECT_MOTION_FORMAT`, `RenderingDevice::DATA_FORMAT_R16G16_SFLOAT`.
pub(crate) const EFFECT_MOTION_FORMAT: i32 = 82;

/// `rsf_godot_effect_frame`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectFrame {
    pub(crate) render_target: u64,
    pub(crate) views: u32,
    pub(crate) internal_width: i32,
    pub(crate) internal_height: i32,
    pub(crate) output_width: i32,
    pub(crate) output_height: i32,
    pub(crate) color_valid: u32,
    pub(crate) depth_valid: u32,
    pub(crate) motion_valid: u32,
    /// Basis rows, then the origin.
    pub(crate) camera_transform: [f64; 12],
    /// Columns.
    pub(crate) jittered_projection: [f64; 16],
}

impl Default for EffectFrame {
    /// The godot-cpp defaults: zero extents, the identity transform and the identity projection.
    fn default() -> Self {
        let mut camera_transform = [0.0; 12];
        let mut jittered_projection = [0.0; 16];
        for axis in 0..3 {
            camera_transform[axis * 3 + axis] = 1.0;
        }
        for axis in 0..4 {
            jittered_projection[axis * 4 + axis] = 1.0;
        }
        Self {
            render_target: 0,
            views: 0,
            internal_width: 0,
            internal_height: 0,
            output_width: 0,
            output_height: 0,
            color_valid: 0,
            depth_valid: 0,
            motion_valid: 0,
            camera_transform,
            jittered_projection,
        }
    }
}

/// `rsf_godot_effect_resources`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectResources {
    pub(crate) color_format: i32,
    pub(crate) depth_format: i32,
    pub(crate) motion_format: i32,
    pub(crate) native_device: u32,
    pub(crate) native_queue: u32,
    pub(crate) native_textures: u32,
}

impl Default for EffectResources {
    /// Format -1 and no native handle, as for buffers that were not admitted.
    fn default() -> Self {
        Self {
            color_format: -1,
            depth_format: -1,
            motion_format: -1,
            native_device: 0,
            native_queue: 0,
            native_textures: 0,
        }
    }
}

/// `rsf_godot_effect_status`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectStatus {
    pub(crate) callbacks: u64,
    pub(crate) frame: EffectFrame,
    pub(crate) resources: EffectResources,
    pub(crate) buffers_available: u32,
    pub(crate) capture_succeeded: u32,
    pub(crate) captures: u64,
    pub(crate) capture_callback: u64,
    pub(crate) capture_pixels: u64,
    pub(crate) capture_moving_pixels: u64,
    pub(crate) capture_nonfinite_pixels: u64,
    pub(crate) capture_unwritten_pixels: u64,
    pub(crate) capture_max_abs_uv: f64,
    pub(crate) stage: *const c_char,
    pub(crate) reason: *const c_char,
    pub(crate) snapshot_scope: *const c_char,
    pub(crate) rendering_ready: u32,
    pub(crate) sr_active: u32,
    pub(crate) fg_active: u32,
    pub(crate) latency_active: u32,
}

/// Recording parameters for the motion capture leaf.
#[repr(C)]
pub(crate) struct EffectReadback {
    pub(crate) shader: *const c_char,
    pub(crate) bytes: u32,
    pub(crate) groups_x: u32,
    pub(crate) groups_y: u32,
}

/// `RSF_GODOT_VK_LOG_*`.
pub(crate) const VK_LOG_REJECTED: u32 = 1;
pub(crate) const VK_LOG_FSR4_DISPATCH: u32 = 2;
pub(crate) const VK_LOG_INPUT_REFUSAL: u32 = 3;

/// `rsf_godot_option_field`.
#[repr(C)]
pub(crate) struct OptionField {
    pub(crate) name: *const c_char,
    pub(crate) conversion: u32,
}

/// `rsf_godot_option_value`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct OptionValue {
    pub(crate) present: u32,
    pub(crate) variant: u32,
    pub(crate) integer: i64,
    pub(crate) real: f64,
}

pub(crate) const CPU_SERVICE_VERSION: u32 = 1;

/// Runtime-owned endpoints in native_cpu_service.h.
#[repr(C)]
pub(crate) struct CpuService {
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
    pub(crate) report: unsafe extern "C" fn(*const rsf_game_cpu_event),
    pub(crate) ping: unsafe extern "C" fn(*mut c_void) -> i32,
    pub(crate) execution: Option<unsafe extern "C" fn(*const rsf_game_render_pass, u32) -> i32>,
}

// The records the engine module and the C leaves also read; runtime_platform.c asserts the same
// layouts. The other records here are Rust to Rust.
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<CpuService>() == 32 && offset_of!(CpuService, execution) == 24);
    assert!(size_of::<Sharpening>() == 56);
    assert!(size_of::<Settings>() == 100);
    assert!(offset_of!(Settings, sharpening) == 44);
    assert!(size_of::<Status>() == 256);
    assert!(offset_of!(Status, sr_result) == 40);
    assert!(offset_of!(Status, evaluated_frames) == 88);
    assert!(offset_of!(Status, presentation_statistics_valid) == 144);
    assert!(offset_of!(Status, window_foreground) == 176);
    assert!(offset_of!(Status, keyboard_inputs) == 192);
    assert!(offset_of!(Status, latency_owner) == 224);
    assert!(offset_of!(Status, derived_boundaries) == 248);
    assert!(size_of::<Setup>() == 48);
    assert!(size_of::<EngineServices>() == 24);
};
#[cfg(test)]
pub(crate) mod fake;

/// The native services this crate calls: the Runtime owners, DXGI and Win32. `host.rs` implements
/// them, and a test fake with the same methods stands in under `cfg(test)`.
pub(crate) struct Platform;
