// SPDX-License-Identifier: MIT
//! `api.h` records and the leaves lent by `runtime.cpp`, mirrored by hand from `api.h` and
//! `godot_platform.h` (bindgen-cli 0.72.1 was not available). Both sides assert sizes and offsets.
//! The leaves carry the D3D12 capture owner, the FG presenter install, header-inline Runtime
//! validators, the Vulkan owner's guard and its device, provider, presenter, SDK and SR context
//! calls in `vulkan.cpp` (the latter driven by `vulkan/sr.rs`), the status facts, and the D3D12
//! device, fence, SR context, FG, presentation and capture calls the `d3d12.rs` owner drives,
//! and the Win32 window, counter, CPU event, ping and present marker calls `window.rs` drives;
//! every decision stays in this crate. The Vulkan creation and compositor effect leaves are
//! separate tables (`VkCreationLeaves`, `EffectLeaves`). The render pass and CPU event are the
//! generated `rsf_game_render_pass` and `rsf_game_cpu_event` bindings. The archive imports no native symbol.
// Mirrored C layouts: the leaves and the engine module read fields this crate only writes.
#![allow(dead_code)]
use rsf_game_sys::{rsf_game_cpu_event, rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::{c_char, c_int, c_void};
use std::sync::OnceLock;

pub(crate) const PLATFORM_VERSION: u32 = 6;
/// `RSF_GODOT_ABI_VERSION`: setup, status, engine services and the Vulkan packet.
pub(crate) const ABI_VERSION: u32 = 3;
/// `RSF_GODOT_SETTINGS_ABI_VERSION`.
pub(crate) const SETTINGS_ABI_VERSION: u32 = 2;

/// `rsf_sharpening_options` (`sharpening_settings.h`). Defaults and validity stay in the Runtime.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Sharpening {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) mode: u32,
    pub(crate) sharpness: f32,
    pub(crate) mas: u32,
    pub(crate) motion_sharpness: f32,
    pub(crate) motion_threshold: f32,
    pub(crate) motion_scale_limit: f32,
    pub(crate) contrast_enabled: u32,
    pub(crate) contrast: f32,
    pub(crate) depth_scale: f32,
    pub(crate) depth_bias: f32,
    pub(crate) clamp_output: u32,
    pub(crate) debug: u32,
}

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
    pub(crate) reserved: u32,
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
    pub(crate) reserved: u32,
    pub(crate) class_name: [u16; 64],
}

impl Default for WindowIdentity {
    fn default() -> Self {
        Self {
            process_id: 0,
            current_process_id: 0,
            class_length: 0,
            reserved: 0,
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
    pub(crate) latency_available: u32,
    pub(crate) latency_mode: u32,
    pub(crate) statistics_flags: u32,
    pub(crate) last_result: i32,
    pub(crate) switch_result: i32,
    pub(crate) backend: u32,
    pub(crate) reserved: u32,
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
    pub(crate) pcl_available: u32,
    pub(crate) report_available: u32,
    pub(crate) owner: u32,
    pub(crate) reserved: u32,
    pub(crate) paced_frames: u64,
    pub(crate) marker_calls: u64,
    pub(crate) inputs: InputFacts,
}

/// One evaluated D3D12 SR pass as `d3d12.rs` reports it to the status record. Formerly the
/// `rsf_godot_sr_frame` record; no leaf reads it any more.
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
    pub(crate) reserved: u32,
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
    pub(crate) reserved: u32,
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
    /// Formerly `xess_device_ready` and `interop_device_ready`; the creation owner
    /// (`vulkan/creation.rs`) keeps both now and the leaf leaves these zero.
    pub(crate) reserved_creation: [u32; 2],
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
    pub(crate) reserved: u32,
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
    pub(crate) fg_result: i32,
}

/// `rsf_godot_vk_fg_pass`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkFgPass {
    pub(crate) role: u32,
    pub(crate) screen: u32,
    pub(crate) session_id: u64,
    pub(crate) view_id: u32,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) enabled: u32,
    pub(crate) generated_frames: u32,
    pub(crate) options_result: i32,
    pub(crate) backend_valid: u32,
    pub(crate) backend: u32,
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
/// Formerly the `rsf_godot_vk_sr_frame` record; no leaf reads it any more.
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

/// `PFN_vkVoidFunction`.
pub(crate) type VkFunction = unsafe extern "C" fn();
/// `rsf_godot_vk_proc`: `vkGetInstanceProcAddr` or `vkGetDeviceProcAddr`.
pub(crate) type VkProc = unsafe extern "C" fn(*mut c_void, *const c_char) -> Option<VkFunction>;

/// `rsf_godot_vk_sr_pass`. `call` is `vulkan.cpp`'s private pass record, lent back to the leaves.
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
    pub(crate) handoff: u32,
    pub(crate) split_record: u32,
    pub(crate) continuation: u32,
    pub(crate) fsr4_compiled: u32,
}

/// `rsf_godot_vk_sr_create`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkSrCreate {
    pub(crate) backend: u32,
    pub(crate) quality: u32,
}

/// `rsf_godot_vk_sr_context`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VkSrContext {
    pub(crate) handle: *mut c_void,
    pub(crate) result: i32,
    pub(crate) reserved: u32,
}

/// `RSF_GODOT_VK_CREATION_VERSION`.
pub(crate) const VK_CREATION_VERSION: u32 = 2;
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
/// `RSF_GODOT_VK_XESS_*`.
pub(crate) const VK_XESS_FEATURES2: u32 = 1;
pub(crate) const VK_XESS_ENABLED_FEATURES: u32 = 2;
pub(crate) const VK_XESS_FLOAT16_INT8: u32 = 4;
pub(crate) const VK_XESS_SCALAR_BLOCK_LAYOUT: u32 = 8;
pub(crate) const VK_XESS_INTEGER_DOT_PRODUCT: u32 = 16;
/// `RSF_GODOT_VK_STAGE_*`.
pub(crate) const VK_STAGE_XESS: u32 = 0;
pub(crate) const VK_STAGE_INTEROP: u32 = 1;
pub(crate) const VK_STAGE_LATENCY: u32 = 2;

/// `rsf_godot_vk_chain_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkChainFacts {
    pub(crate) cloneable: u32,
    pub(crate) features2: u32,
    pub(crate) vulkan12: u32,
    pub(crate) vulkan13: u32,
    pub(crate) float16_int8: u32,
    pub(crate) scalar_block_layout: u32,
    pub(crate) integer_dot_product: u32,
    pub(crate) enabled_features: u32,
    pub(crate) timeline_semaphore: u32,
    pub(crate) reserved: u32,
}

/// `rsf_godot_vk_device_derive`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkDeviceDerive {
    pub(crate) base: *const c_void,
    pub(crate) extensions: *const *const c_char,
    pub(crate) extension_count: u32,
    pub(crate) replace_next: u32,
    pub(crate) next: *mut c_void,
    pub(crate) clear_enabled_features: u32,
    pub(crate) reserved: u32,
}

/// `rsf_godot_vk_adapter`: the created device's physical device, as `vkGetPhysicalDeviceProperties`
/// read it. `software` is a CPU device type.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkAdapter {
    pub(crate) vendor: u32,
    pub(crate) device: u32,
    pub(crate) software: u32,
    pub(crate) reserved: u32,
}

/// `rsf_godot_vk_latency_requirements`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkLatencyRequirements {
    pub(crate) result: i32,
    pub(crate) extension_count: u32,
    pub(crate) extensions: *const *const c_char,
    pub(crate) features: *mut c_void,
}

/// `rsf_godot_vk_instance_call`: one intercepted `vkCreateInstance`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkInstanceCall {
    pub(crate) call: *mut c_void,
    pub(crate) request: *const c_void,
    pub(crate) allocator: *const c_void,
    pub(crate) output: *mut c_void,
    pub(crate) extensions: *const *const c_char,
    pub(crate) extension_count: u32,
    pub(crate) reserved: u32,
}

/// `rsf_godot_vk_device_call`: one intercepted `vkCreateDevice`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkDeviceCall {
    pub(crate) call: *mut c_void,
    pub(crate) physical_device: *mut c_void,
    pub(crate) request: *const c_void,
    pub(crate) allocator: *const c_void,
    pub(crate) output: *mut c_void,
    pub(crate) extensions: *const *const c_char,
    pub(crate) extension_count: u32,
    pub(crate) reserved: u32,
}

/// `rsf_godot_vk_creation_leaves`, the Vulkan-typed leaves of `vulkan_creation.cpp`.
#[repr(C)]
pub(crate) struct VkCreationLeaves {
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
    pub(crate) xess_instance_requirements:
        unsafe extern "C" fn(*mut u32, *mut *const *const c_char, *mut u32) -> i32,
    pub(crate) derive_instance: unsafe extern "C" fn(
        *mut c_void,
        *const c_void,
        u32,
        *const *const c_char,
        u32,
    ) -> *const c_void,
    pub(crate) create_instance:
        unsafe extern "C" fn(VkFunction, *const c_void, *const c_void, *mut c_void) -> i32,
    pub(crate) chain_facts: unsafe extern "C" fn(*const c_void, *mut VkChainFacts),
    pub(crate) xess_features:
        unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut *mut c_void) -> u32,
    pub(crate) xess_device_requirements: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut u32,
        *mut *const *const c_char,
        *mut *mut c_void,
    ) -> i32,
    pub(crate) device_extensions:
        unsafe extern "C" fn(VkFunction, *mut c_void, *const *const c_char, u32) -> u32,
    pub(crate) latency_requirements: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        Option<VkProc>,
        *const c_void,
        *mut VkLatencyRequirements,
    ) -> u32,
    pub(crate) derive_device:
        unsafe extern "C" fn(*mut c_void, u32, *const VkDeviceDerive) -> *const c_void,
    pub(crate) create_device: unsafe extern "C" fn(
        VkFunction,
        *mut c_void,
        *const c_void,
        *const c_void,
        *mut c_void,
    ) -> i32,
    pub(crate) latency_created: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        Option<VkProc>,
        *const c_void,
    ) -> i32,
    pub(crate) latency_destroyed: unsafe extern "C" fn(*mut c_void),
    pub(crate) destroy_device: unsafe extern "C" fn(VkFunction, *mut c_void, *const c_void),
    pub(crate) adapter: unsafe extern "C" fn(VkFunction, *mut c_void, *mut VkAdapter) -> u32,
}

/// `RSF_GODOT_EFFECT_VERSION`: the effect leaves and the effect status record.
pub(crate) const EFFECT_VERSION: u32 = 1;
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
    pub(crate) reserved: [u32; 2],
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
            reserved: [0; 2],
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
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
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

/// `rsf_godot_effect_leaves`, the godot-cpp leaves of `effect.cpp`.
#[repr(C)]
pub(crate) struct EffectLeaves {
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
    pub(crate) frame: unsafe extern "C" fn(*mut c_void, *mut EffectFrame) -> u32,
    pub(crate) resources: unsafe extern "C" fn(*mut c_void, *mut EffectResources) -> u32,
    pub(crate) read_motion: unsafe extern "C" fn(*mut c_void, *mut *const u8, *mut u64) -> u32,
}

/// `RSF_GODOT_VK_LOG_*`.
pub(crate) const VK_LOG_REJECTED: u32 = 1;
pub(crate) const VK_LOG_FSR4_DISPATCH: u32 = 2;
pub(crate) const VK_LOG_INPUT_REFUSAL: u32 = 3;

/// The body `vulkan_locked` runs under the Vulkan owner's guard.
pub(crate) type LockedBody = unsafe extern "C" fn(*mut c_void) -> i32;

/// `rsf_godot_option_field`.
#[repr(C)]
pub(crate) struct OptionField {
    pub(crate) name: *const c_char,
    pub(crate) conversion: u32,
    pub(crate) reserved: u32,
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

/// `rsf_godot_platform`.
#[repr(C)]
pub(crate) struct Platform {
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
    pub(crate) tick_count: unsafe extern "C" fn() -> u64,
    pub(crate) prepare_capture: unsafe extern "C" fn() -> u32,
    pub(crate) fg_install: unsafe extern "C" fn(*const FgInstall) -> i32,
    pub(crate) fg_options: unsafe extern "C" fn(*const FgOptions),
    pub(crate) dlss_preset_valid: unsafe extern "C" fn(u32) -> u32,
    pub(crate) sharpening_valid: unsafe extern "C" fn(*const Sharpening) -> u32,
    pub(crate) sharpening_defaults: unsafe extern "C" fn(*mut Sharpening),
    pub(crate) fg_backend_known: unsafe extern "C" fn(u32) -> u32,
    pub(crate) vulkan_supported: unsafe extern "C" fn() -> u32,
    pub(crate) vulkan_latency_facts: unsafe extern "C" fn(*mut VkLatencyFacts) -> u32,
    pub(crate) vulkan_locked: unsafe extern "C" fn(LockedBody, *mut c_void) -> i32,
    pub(crate) vulkan_start: unsafe extern "C" fn(*const Setup) -> i32,
    pub(crate) vulkan_stop: unsafe extern "C" fn() -> i32,
    pub(crate) vulkan_release: unsafe extern "C" fn(u64),
    pub(crate) vulkan_facts: unsafe extern "C" fn(*mut VkFacts),
    pub(crate) vulkan_presenter_max: unsafe extern "C" fn(*mut u32) -> i32,
    pub(crate) vulkan_fsr4_fg_probe: unsafe extern "C" fn() -> u32,
    pub(crate) vulkan_latency_options: unsafe extern "C" fn(u32, u32) -> i32,
    pub(crate) cpu_counters: unsafe extern "C" fn(*mut InputFacts),
    pub(crate) window_facts: unsafe extern "C" fn(*mut c_void, *mut WindowFacts),
    pub(crate) fg_facts: unsafe extern "C" fn(*mut FgFacts),
    pub(crate) latency_facts: unsafe extern "C" fn(*mut LatencyFacts) -> u32,
    pub(crate) d3d12_bind: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    pub(crate) d3d12_wait: unsafe extern "C" fn(u64) -> u32,
    pub(crate) d3d12_signal: unsafe extern "C" fn(u64) -> u32,
    pub(crate) sr_default_backend: unsafe extern "C" fn() -> u32,
    pub(crate) fg_default_backend: unsafe extern "C" fn() -> u32,
    pub(crate) present_owner: unsafe extern "C" fn(*mut u32) -> u32,
    pub(crate) present_request: unsafe extern "C" fn(u32),
    pub(crate) sr_create: unsafe extern "C" fn(*const SrCreate, *mut SrContext) -> u32,
    pub(crate) sr_destroy: unsafe extern "C" fn(*mut c_void),
    pub(crate) sr_evaluate: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *const rsf_game_render_pass,
        *const SrEvaluate,
        *mut i32,
    ) -> u32,
    pub(crate) sr_refusal_log: unsafe extern "C" fn(*const rsf_game_render_pass, u32, u32),
    pub(crate) capture_sr_begin: unsafe extern "C" fn(*const rsf_game_render_pass) -> u32,
    pub(crate) final_scene:
        unsafe extern "C" fn(*mut c_void, *const rsf_game_render_pass, u32) -> u32,
    pub(crate) fg_submitted: unsafe extern "C" fn(u64, u64),
    pub(crate) render_begin: unsafe extern "C" fn(u64, u64) -> u32,
    pub(crate) present_window: unsafe extern "C" fn(*const rsf_game_render_pass),
    pub(crate) fg_end_session: unsafe extern "C" fn(u64) -> u32,
    pub(crate) d3d12_release: unsafe extern "C" fn(*mut c_void, u64) -> u32,
    pub(crate) vulkan_presenter_drain: unsafe extern "C" fn(u32) -> i32,
    pub(crate) vulkan_device_idle: unsafe extern "C" fn(*mut c_void, Option<VkProc>) -> u32,
    pub(crate) vulkan_sr_destroy: unsafe extern "C" fn(*mut c_void) -> i32,
    pub(crate) vulkan_fsr4_destroy: unsafe extern "C" fn(*mut c_void) -> i32,
    pub(crate) vulkan_sr_create:
        unsafe extern "C" fn(*mut c_void, *const VkSrCreate, *mut VkSrContext) -> u32,
    pub(crate) vulkan_fsr4_create:
        unsafe extern "C" fn(*mut c_void, *const VkSrCreate, *mut VkSrContext) -> u32,
    pub(crate) vulkan_dlss_preset: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    pub(crate) vulkan_sr_plan:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u32, *mut u32) -> u32,
    pub(crate) vulkan_fsr4_plan:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u32, *mut u32) -> u32,
    pub(crate) vulkan_sr_evaluate:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *const Sharpening, *mut i32) -> u32,
    pub(crate) vulkan_fsr4_evaluate: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *const Sharpening,
        *mut i32,
        *mut *mut c_void,
    ) -> u32,
    pub(crate) vulkan_sr_fallback: unsafe extern "C" fn(*mut c_void, *mut i32) -> u32,
    pub(crate) vulkan_sr_log: unsafe extern "C" fn(*mut c_void, u32, i32, i32) -> u32,
    pub(crate) vulkan_sr_dump_result: unsafe extern "C" fn(*mut c_void, u32, i32) -> u32,
    pub(crate) window_identity: unsafe extern "C" fn(*mut c_void, *mut WindowIdentity),
    pub(crate) window_procedure_get: unsafe extern "C" fn(*mut c_void) -> isize,
    pub(crate) window_procedure_set: unsafe extern "C" fn(*mut c_void, isize) -> isize,
    pub(crate) window_forward: unsafe extern "C" fn(isize, *mut c_void, u32, usize, isize) -> isize,
    pub(crate) query_counter: unsafe extern "C" fn(*mut u64, *mut u64),
    pub(crate) cpu_event: unsafe extern "C" fn(*const rsf_game_cpu_event),
    pub(crate) vulkan_cpu_event: unsafe extern "C" fn(*const rsf_game_cpu_event),
    pub(crate) latency_ping: unsafe extern "C" fn(*mut c_void) -> i32,
    pub(crate) vulkan_latency_ping: unsafe extern "C" fn(*mut c_void) -> i32,
    pub(crate) vulkan_present_marker: unsafe extern "C" fn(u32),
    pub(crate) sr_select_backend: unsafe extern "C" fn(u32, u32, u32) -> u32,
    pub(crate) fg_select_backend: unsafe extern "C" fn(u32, u32, u32) -> u32,
}

const _: () = {
    use std::mem::{offset_of, size_of};
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
    assert!(size_of::<FgOptions>() == 20);
    assert!(size_of::<FgInstall>() == 96);
    assert!(offset_of!(FgInstall, fidelityfx_directory) == 56);
    assert!(size_of::<Paths>() == 56);
    assert!(size_of::<InputFacts>() == 56);
    assert!(size_of::<WindowFacts>() == 16);
    assert!(size_of::<FgFacts>() == 96);
    assert!(offset_of!(FgFacts, tagged_frames) == 48);
    assert!(size_of::<LatencyFacts>() == 88);
    assert!(size_of::<SrCreate>() == 88);
    assert!(offset_of!(SrCreate, paths) == 32);
    assert!(size_of::<SrContext>() == 24);
    assert!(offset_of!(SrContext, desired_width) == 12);
    assert!(size_of::<SrEvaluate>() == 64);
    assert!(size_of::<OptionField>() == 16);
    assert!(size_of::<OptionValue>() == 24);
    assert!(size_of::<VkFacts>() == 40);
    assert!(size_of::<VkLatencyFacts>() == 40);
    assert!(offset_of!(VkLatencyFacts, sleep_calls) == 24);
    assert!(size_of::<VkView>() == 136);
    assert!(offset_of!(VkView, generation) == 104);
    assert!(offset_of!(VkView, fg_result) == 132);
    assert!(size_of::<VkFgPass>() == 48);
    assert!(offset_of!(VkFgPass, options_result) == 36);
    assert!(size_of::<VkPresentFacts>() == 32);
    assert!(size_of::<VkSrPass>() == 72);
    assert!(offset_of!(VkSrPass, output_width) == 32);
    assert!(offset_of!(VkSrPass, source_frame_id) == 40);
    assert!(offset_of!(VkSrPass, handoff) == 56);
    assert!(size_of::<VkSrCreate>() == 8);
    assert!(size_of::<VkSrContext>() == 16);
    assert!(offset_of!(VkSrContext, result) == 8);
    assert!(offset_of!(VkFacts, graph_handoff) == 8);
    assert!(size_of::<VkChainFacts>() == 40);
    assert!(size_of::<VkDeviceDerive>() == 40);
    assert!(offset_of!(VkDeviceDerive, next) == 24);
    assert!(size_of::<VkLatencyRequirements>() == 24);
    assert!(size_of::<VkInstanceCall>() == 48);
    assert!(size_of::<VkDeviceCall>() == 56);
    assert!(offset_of!(VkDeviceCall, extension_count) == 48);
    assert!(size_of::<VkCreationLeaves>() == 120);
    assert!(offset_of!(VkCreationLeaves, latency_requirements) == 64);
    assert!(offset_of!(VkCreationLeaves, destroy_device) == 104);
    assert!(offset_of!(VkCreationLeaves, adapter) == 112);
    assert!(size_of::<VkAdapter>() == 16);
    assert!(size_of::<EffectFrame>() == 272);
    assert!(offset_of!(EffectFrame, motion_valid) == 36);
    assert!(offset_of!(EffectFrame, camera_transform) == 48);
    assert!(offset_of!(EffectFrame, jittered_projection) == 144);
    assert!(size_of::<EffectResources>() == 24);
    assert!(size_of::<EffectStatus>() == 416);
    assert!(offset_of!(EffectStatus, frame) == 16);
    assert!(offset_of!(EffectStatus, resources) == 288);
    assert!(offset_of!(EffectStatus, buffers_available) == 312);
    assert!(offset_of!(EffectStatus, captures) == 320);
    assert!(offset_of!(EffectStatus, capture_max_abs_uv) == 368);
    assert!(offset_of!(EffectStatus, stage) == 376);
    assert!(offset_of!(EffectStatus, rendering_ready) == 400);
    assert!(size_of::<EffectLeaves>() == 32);
    assert!(offset_of!(EffectLeaves, read_motion) == 24);
    assert!(size_of::<WindowIdentity>() == 144);
    assert!(offset_of!(WindowIdentity, class_name) == 16);
    assert!(size_of::<Platform>() == 536);
    assert!(offset_of!(Platform, sr_select_backend) == 520);
    assert!(offset_of!(Platform, vulkan_locked) == 88);
    assert!(offset_of!(Platform, latency_facts) == 176);
    assert!(offset_of!(Platform, d3d12_bind) == 184);
    assert!(offset_of!(Platform, sr_evaluate) == 256);
    assert!(offset_of!(Platform, d3d12_release) == 320);
    assert!(offset_of!(Platform, vulkan_presenter_drain) == 328);
    assert!(offset_of!(Platform, vulkan_sr_create) == 360);
    assert!(offset_of!(Platform, vulkan_sr_evaluate) == 400);
    assert!(offset_of!(Platform, vulkan_sr_dump_result) == 432);
    assert!(offset_of!(Platform, window_identity) == 440);
    assert!(offset_of!(Platform, cpu_event) == 480);
    assert!(offset_of!(Platform, vulkan_present_marker) == 512);
};

#[cfg(test)]
pub(crate) mod fake;

static PLATFORM: OnceLock<&'static Platform> = OnceLock::new();

/// Keeps the first complete table.
///
/// # Safety
/// `table` is null or a process-lived `rsf_godot_platform`.
pub(crate) unsafe fn register(table: *const Platform) -> Option<&'static Platform> {
    if let Some(platform) = PLATFORM.get() {
        return Some(platform);
    }
    // SAFETY: the caller passes null or a table that lives for the process.
    let table: &'static Platform = unsafe { table.as_ref() }?;
    (table.struct_size as usize >= size_of::<Platform>() && table.version == PLATFORM_VERSION)
        .then(|| *PLATFORM.get_or_init(|| table))
}

/// The table kept by `register`, for the window procedure, which has no table argument.
pub(crate) fn registered() -> Option<&'static Platform> {
    PLATFORM.get().copied()
}

// Each leaf is a complete process-lived entry (register) and catches its own C++ exceptions.
// SAFETY for every unsafe block below: the entry is valid and its pointer arguments are live
// for the call.
impl Platform {
    pub(crate) fn tick_count(&self) -> u64 {
        unsafe { (self.tick_count)() }
    }

    pub(crate) fn prepare_capture(&self) -> bool {
        unsafe { (self.prepare_capture)() != 0 }
    }

    pub(crate) fn fg_install(&self, install: &FgInstall) -> i32 {
        unsafe { (self.fg_install)(install) }
    }

    pub(crate) fn fg_options(&self, options: &FgOptions) {
        unsafe { (self.fg_options)(options) }
    }

    pub(crate) fn dlss_preset_valid(&self, preset: u32) -> bool {
        unsafe { (self.dlss_preset_valid)(preset) != 0 }
    }

    pub(crate) fn sharpening_valid(&self, options: &Sharpening) -> bool {
        unsafe { (self.sharpening_valid)(options) != 0 }
    }

    pub(crate) fn sharpening_defaults(&self) -> Sharpening {
        let mut options = Sharpening::default();
        unsafe { (self.sharpening_defaults)(&mut options) };
        options
    }

    pub(crate) fn fg_backend_known(&self, backend: u32) -> bool {
        unsafe { (self.fg_backend_known)(backend) != 0 }
    }

    pub(crate) fn vulkan_supported(&self) -> bool {
        unsafe { (self.vulkan_supported)() != 0 }
    }

    /// The Streamline Vulkan latency status; none when the Runtime has none.
    pub(crate) fn vulkan_latency_facts(&self) -> Option<VkLatencyFacts> {
        let mut facts = VkLatencyFacts::default();
        (unsafe { (self.vulkan_latency_facts)(&mut facts) } != 0).then_some(facts)
    }

    /// Runs `body` with the Vulkan owner's guard held and returns its result, or zero when the
    /// leaf could not take the guard. The `vulkan_*` calls below are made only inside `body`.
    /// The guard is not reentrant: `body` never calls back into a guarded `rsfg_vk_*` path.
    pub(crate) fn vulkan_locked<F: FnMut() -> c_int>(&self, mut body: F) -> c_int {
        unsafe extern "C" fn run<F: FnMut() -> c_int>(user: *mut c_void) -> i32 {
            // SAFETY: `user` is the closure below, live and exclusively borrowed for the call.
            let body = unsafe { &mut *user.cast::<F>() };
            body()
        }
        unsafe { (self.vulkan_locked)(run::<F>, (&raw mut body).cast()) }
    }

    /// Directories, Streamline and XeSS initialization and the creation resolver. Zero refuses.
    pub(crate) fn vulkan_start(&self, setup: &Setup) -> c_int {
        unsafe { (self.vulkan_start)(setup) }
    }

    /// Presenter disable and drain, SR context and capture retirement, XeSS shutdown and the SDK
    /// shutdown or its deferral to presenter retirement. Zero refuses and keeps the session.
    pub(crate) fn vulkan_stop(&self) -> c_int {
        unsafe { (self.vulkan_stop)() }
    }

    /// Cancels the session's frame dump and drops the device borrow.
    pub(crate) fn vulkan_release(&self, session_id: u64) {
        unsafe { (self.vulkan_release)(session_id) }
    }

    pub(crate) fn vulkan_facts(&self) -> VkFacts {
        let mut facts = VkFacts::default();
        unsafe { (self.vulkan_facts)(&mut facts) };
        facts
    }

    /// The live presenter's generated-frame capacity, when it answers.
    pub(crate) fn vulkan_presenter_max(&self) -> Option<u32> {
        let mut count = 0;
        (unsafe { (self.vulkan_presenter_max)(&mut count) } == 0).then_some(count)
    }

    /// The FSR4 FG generated-frame capacity on the presenter's device; zero when the probe
    /// refuses.
    pub(crate) fn vulkan_fsr4_fg_probe(&self) -> u32 {
        unsafe { (self.vulkan_fsr4_fg_probe)() }
    }

    pub(crate) fn vulkan_latency_options(&self, mode: u32, frame_limit_us: u32) -> i32 {
        unsafe { (self.vulkan_latency_options)(mode, frame_limit_us) }
    }

    pub(crate) fn cpu_counters(&self) -> InputFacts {
        let mut facts = InputFacts::default();
        unsafe { (self.cpu_counters)(&mut facts) };
        facts
    }

    /// Foreground process and the visibility of `window`, the admitted input window or null.
    pub(crate) fn window_facts(&self, window: *mut c_void) -> WindowFacts {
        let mut facts = WindowFacts::default();
        unsafe { (self.window_facts)(window, &mut facts) };
        facts
    }

    pub(crate) fn fg_facts(&self) -> FgFacts {
        let mut facts = FgFacts::default();
        unsafe { (self.fg_facts)(&mut facts) };
        facts
    }

    pub(crate) fn latency_facts(&self) -> Option<LatencyFacts> {
        let mut facts = LatencyFacts::default();
        (unsafe { (self.latency_facts)(&mut facts) } != 0).then_some(facts)
    }

    // The D3D12 SR context and submission leaves. Device, queue, command list and context
    // pointers are the engine's or the Runtime's live objects for the call; this crate only
    // compares and passes them. A `false` or `None` completion means the leaf caught an exception.

    /// Keeps the engine's device and queue and creates the fence; false when the fence failed.
    pub(crate) fn d3d12_bind(&self, device: *mut c_void, queue: *mut c_void) -> bool {
        unsafe { (self.d3d12_bind)(device, queue) != 0 }
    }

    /// The fence reached `value`, or there is nothing to wait for. False after 15 s.
    pub(crate) fn d3d12_wait(&self, value: u64) -> bool {
        unsafe { (self.d3d12_wait)(value) != 0 }
    }

    pub(crate) fn d3d12_signal(&self, value: u64) -> bool {
        unsafe { (self.d3d12_signal)(value) != 0 }
    }

    /// `rsf_gpu_default_sr_backend` for the bound adapter.
    pub(crate) fn sr_default_backend(&self) -> u32 {
        unsafe { (self.sr_default_backend)() }
    }

    /// `rsf_fg_default_backend` for the bound adapter.
    pub(crate) fn fg_default_backend(&self) -> u32 {
        unsafe { (self.fg_default_backend)() }
    }

    /// The presentation provider's backend while it has an owner.
    pub(crate) fn present_owner(&self) -> Option<u32> {
        let mut backend = 0;
        (unsafe { (self.present_owner)(&mut backend) } != 0).then_some(backend)
    }

    pub(crate) fn present_request(&self, backend: u32) {
        unsafe { (self.present_request)(backend) }
    }

    /// `desired` is the previous plan, which stays when no context is made.
    pub(crate) fn sr_create(&self, request: &SrCreate, desired: (u32, u32)) -> Option<SrContext> {
        let mut context = SrContext {
            handle: std::ptr::null_mut(),
            result: 0,
            desired_width: desired.0,
            desired_height: desired.1,
            reserved: 0,
        };
        (unsafe { (self.sr_create)(request, &mut context) } != 0).then_some(context)
    }

    pub(crate) fn sr_destroy(&self, context: *mut c_void) {
        unsafe { (self.sr_destroy)(context) }
    }

    pub(crate) fn sr_evaluate(
        &self,
        context: *mut c_void,
        commands: *mut c_void,
        pass: &rsf_game_render_pass,
        request: &SrEvaluate,
    ) -> Option<i32> {
        let mut result = 0;
        (unsafe { (self.sr_evaluate)(context, commands, pass, request, &mut result) } != 0)
            .then_some(result)
    }

    pub(crate) fn sr_refusal_log(&self, pass: &rsf_game_render_pass, desired: (u32, u32)) {
        unsafe { (self.sr_refusal_log)(pass, desired.0, desired.1) }
    }

    pub(crate) fn capture_sr_begin(&self, pass: &rsf_game_render_pass) -> bool {
        unsafe { (self.capture_sr_begin)(pass) != 0 }
    }

    pub(crate) fn final_scene(
        &self,
        commands: *mut c_void,
        pass: &rsf_game_render_pass,
        slot: u32,
    ) -> bool {
        unsafe { (self.final_scene)(commands, pass, slot) != 0 }
    }

    pub(crate) fn fg_submitted(&self, session_id: u64, frame_id: u64) {
        unsafe { (self.fg_submitted)(session_id, frame_id) }
    }

    pub(crate) fn render_begin(&self, session_id: u64, frame_id: u64) -> bool {
        unsafe { (self.render_begin)(session_id, frame_id) != 0 }
    }

    pub(crate) fn present_window(&self, pass: &rsf_game_render_pass) {
        unsafe { (self.present_window)(pass) }
    }

    pub(crate) fn fg_end_session(&self, session_id: u64) -> bool {
        unsafe { (self.fg_end_session)(session_id) != 0 }
    }

    /// Capture flush, context destroy (null accepted) and the session's frame dump cancel.
    pub(crate) fn d3d12_release(&self, context: *mut c_void, session_id: u64) -> bool {
        unsafe { (self.d3d12_release)(context, session_id) != 0 }
    }

    // The Vulkan SR context leaves, called only while `vulkan.cpp` holds its guard. Context
    // handles are the Runtime's, held here as addresses; `call` is the leaf's live pass record for
    // the call. A `None` completion means the leaf caught an exception.

    /// Drains the live presenter, disabling it first when asked; `BACKEND_OK` without one.
    pub(crate) fn vulkan_presenter_drain(&self, disable: bool) -> i32 {
        unsafe { (self.vulkan_presenter_drain)(u32::from(disable)) }
    }

    /// `vkDeviceWaitIdle` on the device the contexts were created for.
    pub(crate) fn vulkan_device_idle(&self, device: *mut c_void, proc: Option<VkProc>) -> bool {
        unsafe { (self.vulkan_device_idle)(device, proc) != 0 }
    }

    pub(crate) fn vulkan_sr_destroy(&self, context: *mut c_void) -> i32 {
        unsafe { (self.vulkan_sr_destroy)(context) }
    }

    /// The FSR4 bridge code; zero is success.
    pub(crate) fn vulkan_fsr4_destroy(&self, context: *mut c_void) -> i32 {
        unsafe { (self.vulkan_fsr4_destroy)(context) }
    }

    /// The created record, also when the leaf did not complete, so a handle it wrote is kept.
    pub(crate) fn vulkan_sr_create(
        &self,
        call: *mut c_void,
        request: &VkSrCreate,
        fsr4: bool,
    ) -> (bool, VkSrContext) {
        let mut created = VkSrContext {
            handle: std::ptr::null_mut(),
            result: 0,
            reserved: 0,
        };
        let leaf = if fsr4 {
            self.vulkan_fsr4_create
        } else {
            self.vulkan_sr_create
        };
        let completed = unsafe { leaf(call, request, &mut created) } != 0;
        (completed, created)
    }

    pub(crate) fn vulkan_dlss_preset(&self, context: *mut c_void, preset: u32) -> Option<()> {
        (unsafe { (self.vulkan_dlss_preset)(context, preset) } != 0).then_some(())
    }

    /// The context's planned input extent.
    pub(crate) fn vulkan_sr_plan(
        &self,
        call: *mut c_void,
        context: *mut c_void,
        fsr4: bool,
    ) -> Option<(u32, u32)> {
        let (mut width, mut height) = (0, 0);
        let leaf = if fsr4 {
            self.vulkan_fsr4_plan
        } else {
            self.vulkan_sr_plan
        };
        (unsafe { leaf(call, context, &mut width, &mut height) } != 0).then_some((width, height))
    }

    pub(crate) fn vulkan_sr_evaluate(
        &self,
        call: *mut c_void,
        context: *mut c_void,
        sharpening: &Sharpening,
    ) -> Option<i32> {
        let mut result = 0;
        (unsafe { (self.vulkan_sr_evaluate)(call, context, sharpening, &mut result) } != 0)
            .then_some(result)
    }

    /// The result and the command buffer recording continues on.
    pub(crate) fn vulkan_fsr4_evaluate(
        &self,
        call: *mut c_void,
        context: *mut c_void,
        sharpening: &Sharpening,
    ) -> Option<(i32, *mut c_void)> {
        let (mut result, mut next) = (0, std::ptr::null_mut());
        (unsafe { (self.vulkan_fsr4_evaluate)(call, context, sharpening, &mut result, &mut next) }
            != 0)
            .then_some((result, next))
    }

    pub(crate) fn vulkan_sr_fallback(&self, call: *mut c_void) -> Option<i32> {
        let mut result = 0;
        (unsafe { (self.vulkan_sr_fallback)(call, &mut result) } != 0).then_some(result)
    }

    pub(crate) fn vulkan_sr_log(
        &self,
        call: *mut c_void,
        kind: u32,
        provider: i32,
        fallback: i32,
    ) -> Option<()> {
        (unsafe { (self.vulkan_sr_log)(call, kind, provider, fallback) } != 0).then_some(())
    }

    pub(crate) fn vulkan_sr_dump_result(
        &self,
        call: *mut c_void,
        backend: u32,
        result: i32,
    ) -> Option<()> {
        (unsafe { (self.vulkan_sr_dump_result)(call, backend, result) } != 0).then_some(())
    }

    // The Win32 window and input leaves (`window.rs`). Windows are handles passed through; a
    // procedure is a `WNDPROC` address as `GWLP_WNDPROC` stores it.

    pub(crate) fn window_identity(&self, window: *mut c_void) -> WindowIdentity {
        let mut identity = WindowIdentity::default();
        unsafe { (self.window_identity)(window, &mut identity) };
        identity
    }

    /// `GetWindowLongPtrW(GWLP_WNDPROC)`; zero on failure.
    pub(crate) fn window_procedure_get(&self, window: *mut c_void) -> isize {
        unsafe { (self.window_procedure_get)(window) }
    }

    /// `SetWindowLongPtrW(GWLP_WNDPROC)`: the replaced procedure, zero on failure.
    pub(crate) fn window_procedure_set(&self, window: *mut c_void, procedure: isize) -> isize {
        unsafe { (self.window_procedure_set)(window, procedure) }
    }

    /// `CallWindowProcW` of `previous`, or `DefWindowProcW` when it is zero.
    pub(crate) fn window_forward(
        &self,
        previous: isize,
        window: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        unsafe { (self.window_forward)(previous, window, message, wparam, lparam) }
    }

    /// `QueryPerformanceCounter` and `QueryPerformanceFrequency`.
    pub(crate) fn query_counter(&self) -> (u64, u64) {
        let (mut counter, mut frequency) = (0, 0);
        unsafe { (self.query_counter)(&mut counter, &mut frequency) };
        (counter, frequency)
    }

    /// `rsf_latencyvk_cpu` for a Vulkan session, otherwise `rsf_fg12_cpu`.
    pub(crate) fn cpu_event(&self, vulkan: bool, event: &rsf_game_cpu_event) {
        let leaf = if vulkan {
            self.vulkan_cpu_event
        } else {
            self.cpu_event
        };
        unsafe { leaf(event) }
    }

    /// `rsf_latencyvk_ping` for a Vulkan session, otherwise `rsf_native_latency_ping`.
    pub(crate) fn latency_ping(&self, vulkan: bool, window: *mut c_void) -> c_int {
        let leaf = if vulkan {
            self.vulkan_latency_ping
        } else {
            self.latency_ping
        };
        unsafe { leaf(window) }
    }

    /// `rsf_latencyvk_present`.
    pub(crate) fn vulkan_present_marker(&self, begin: u32) {
        unsafe { (self.vulkan_present_marker)(begin) }
    }

    /// `rsf_gpu_select_sr_backend`: the Runtime's SR Auto choice among `choices`.
    pub(crate) fn sr_select_backend(&self, vendor: u32, software: u32, choices: u32) -> u32 {
        unsafe { (self.sr_select_backend)(vendor, software, choices) }
    }

    /// `rsf_fg_select_auto_backend` of `rsf_fg_default_backend`: the Runtime's FG Auto choice
    /// among `choices`.
    pub(crate) fn fg_select_backend(&self, vendor: u32, software: u32, choices: u32) -> u32 {
        unsafe { (self.fg_select_backend)(vendor, software, choices) }
    }
}

/// The creation table from `vulkan_creation.cpp`, when its header matches.
///
/// # Safety
/// `table` is null or a process-lived `rsf_godot_vk_creation_leaves`.
pub(crate) unsafe fn creation_leaves(
    table: *const VkCreationLeaves,
) -> Option<&'static VkCreationLeaves> {
    // SAFETY: the caller passes null or a table that lives for the process.
    let table: &'static VkCreationLeaves = unsafe { table.as_ref() }?;
    (table.struct_size as usize >= size_of::<VkCreationLeaves>()
        && table.version == VK_CREATION_VERSION)
        .then_some(table)
}

// Each creation leaf is a complete entry of a checked table and catches its own C++ exceptions.
// SAFETY for every unsafe block below: the entry is valid; requests, call records, allocators,
// outputs and handles are the entry point's live arguments for the call, and extension lists are
// readable for their counts while the call lasts.
impl VkCreationLeaves {
    /// The XeSS instance extensions and minimum API version, when the SDK answers.
    pub(crate) fn xess_instance_requirements(&self) -> Option<(&[*const c_char], u32)> {
        let (mut count, mut minimum) = (0, 0);
        let mut names: *const *const c_char = std::ptr::null();
        let result =
            unsafe { (self.xess_instance_requirements)(&mut count, &mut names, &mut minimum) };
        // SAFETY: the SDK's names stay valid until shutdown.
        (result == 0).then(|| (unsafe { extension_list(names, count) }, minimum))
    }

    pub(crate) fn derive_instance(
        &self,
        call: *mut c_void,
        base: *const c_void,
        minimum_api: u32,
        extensions: &[*const c_char],
    ) -> *const c_void {
        unsafe {
            (self.derive_instance)(
                call,
                base,
                minimum_api,
                extensions.as_ptr(),
                extensions.len() as u32,
            )
        }
    }

    pub(crate) fn create_instance(
        &self,
        create: VkFunction,
        request: *const c_void,
        allocator: *const c_void,
        output: *mut c_void,
    ) -> i32 {
        unsafe { (self.create_instance)(create, request, allocator, output) }
    }

    pub(crate) fn chain_facts(&self, request: *const c_void) -> VkChainFacts {
        let mut facts = VkChainFacts::default();
        unsafe { (self.chain_facts)(request, &mut facts) };
        facts
    }

    /// The patched clone's head, null when the clone refused; `None` when the leaf threw.
    pub(crate) fn xess_features(
        &self,
        call: *mut c_void,
        request: *const c_void,
        structures: u32,
    ) -> Option<*mut c_void> {
        let mut features = std::ptr::null_mut();
        (unsafe { (self.xess_features)(call, request, structures, &mut features) } != 0)
            .then_some(features)
    }

    /// The XeSS device extensions and the chain to create with, when the SDK answers.
    pub(crate) fn xess_device_requirements(
        &self,
        instance: *mut c_void,
        physical_device: *mut c_void,
        features: *mut c_void,
    ) -> Option<(&[*const c_char], *mut c_void)> {
        let (mut count, mut features) = (0, features);
        let mut names: *const *const c_char = std::ptr::null();
        let result = unsafe {
            (self.xess_device_requirements)(
                instance,
                physical_device,
                &mut count,
                &mut names,
                &mut features,
            )
        };
        // SAFETY: the SDK's names stay valid until shutdown.
        (result == 0).then(|| (unsafe { extension_list(names, count) }, features))
    }

    pub(crate) fn device_extensions(
        &self,
        enumerate: VkFunction,
        physical_device: *mut c_void,
        names: &[*const c_char],
    ) -> bool {
        unsafe {
            (self.device_extensions)(
                enumerate,
                physical_device,
                names.as_ptr(),
                names.len() as u32,
            ) != 0
        }
    }

    /// `None` when the leaf threw.
    pub(crate) fn latency_requirements(
        &self,
        call: *mut c_void,
        instance: *mut c_void,
        physical_device: *mut c_void,
        instance_proc: Option<VkProc>,
        request: *const c_void,
    ) -> Option<VkLatencyRequirements> {
        let mut requirements = VkLatencyRequirements {
            result: 0,
            extension_count: 0,
            extensions: std::ptr::null(),
            features: std::ptr::null_mut(),
        };
        (unsafe {
            (self.latency_requirements)(
                call,
                instance,
                physical_device,
                instance_proc,
                request,
                &mut requirements,
            )
        } != 0)
            .then_some(requirements)
    }

    pub(crate) fn derive_device(
        &self,
        call: *mut c_void,
        stage: u32,
        derive: &VkDeviceDerive,
    ) -> *const c_void {
        unsafe { (self.derive_device)(call, stage, derive) }
    }

    pub(crate) fn create_device(
        &self,
        create: VkFunction,
        physical_device: *mut c_void,
        request: *const c_void,
        allocator: *const c_void,
        output: *mut c_void,
    ) -> i32 {
        unsafe { (self.create_device)(create, physical_device, request, allocator, output) }
    }

    pub(crate) fn latency_created(
        &self,
        instance: *mut c_void,
        physical_device: *mut c_void,
        device: *mut c_void,
        instance_proc: Option<VkProc>,
        request: *const c_void,
    ) -> i32 {
        unsafe { (self.latency_created)(instance, physical_device, device, instance_proc, request) }
    }

    pub(crate) fn latency_destroyed(&self, device: *mut c_void) {
        unsafe { (self.latency_destroyed)(device) }
    }

    pub(crate) fn destroy_device(
        &self,
        destroy: VkFunction,
        device: *mut c_void,
        allocator: *const c_void,
    ) {
        unsafe { (self.destroy_device)(destroy, device, allocator) }
    }

    /// `properties` is `vkGetPhysicalDeviceProperties`; `None` when the leaf threw.
    pub(crate) fn adapter(
        &self,
        properties: VkFunction,
        physical_device: *mut c_void,
    ) -> Option<VkAdapter> {
        let mut adapter = VkAdapter::default();
        (unsafe { (self.adapter)(properties, physical_device, &mut adapter) } != 0)
            .then_some(adapter)
    }
}

/// The effect table from `effect.cpp`, when its header matches.
///
/// # Safety
/// `table` is null or a process-lived `rsf_godot_effect_leaves`.
pub(crate) unsafe fn effect_leaves(table: *const EffectLeaves) -> Option<&'static EffectLeaves> {
    // SAFETY: the caller passes null or a table that lives for the process.
    let table: &'static EffectLeaves = unsafe { table.as_ref() }?;
    (table.struct_size as usize >= size_of::<EffectLeaves>() && table.version == EFFECT_VERSION)
        .then_some(table)
}

// Each effect leaf is a complete entry of a checked table and catches its own C++ exceptions.
// SAFETY for every unsafe block below: the entry is valid and `call` is the render callback's live
// record, which outlives the owner's render call.
impl EffectLeaves {
    /// The frame facts; `None` without a RenderingDevice, scene buffers or scene data, or when the
    /// leaf threw.
    pub(crate) fn frame(&self, call: *mut c_void) -> Option<EffectFrame> {
        let mut frame = EffectFrame::default();
        (unsafe { (self.frame)(call, &mut frame) } != 0).then_some(frame)
    }

    /// The formats and native handle facts; the defaults when the leaf threw.
    pub(crate) fn resources(&self, call: *mut c_void) -> EffectResources {
        let mut resources = EffectResources::default();
        if unsafe { (self.resources)(call, &mut resources) } == 0 {
            resources = EffectResources::default();
        }
        resources
    }

    /// Runs `read` over the readback bytes, which `call` keeps alive until the callback ends.
    /// An empty slice when the readback was refused or the leaf threw.
    pub(crate) fn read_motion<R>(&self, call: *mut c_void, read: impl FnOnce(&[u8]) -> R) -> R {
        let (mut data, mut size): (*const u8, u64) = (std::ptr::null(), 0);
        let completed = unsafe { (self.read_motion)(call, &mut data, &mut size) } != 0;
        if !completed || data.is_null() || size == 0 {
            return read(&[]);
        }
        // SAFETY: the leaf's bytes stay in `call` for the callback.
        read(unsafe { std::slice::from_raw_parts(data, size as usize) })
    }
}

/// An extension list as a slice; empty for a zero count or a null list.
///
/// # Safety
/// `names` is null or readable for `count` entries for the returned lifetime.
pub(crate) unsafe fn extension_list<'a>(
    names: *const *const c_char,
    count: u32,
) -> &'a [*const c_char] {
    if count == 0 || names.is_null() {
        &[]
    } else {
        // SAFETY: the caller's guarantee.
        unsafe { std::slice::from_raw_parts(names, count as usize) }
    }
}
