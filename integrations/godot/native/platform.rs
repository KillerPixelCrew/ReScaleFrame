// SPDX-License-Identifier: MIT
//! `api.h` records and the leaves lent by `runtime.cpp`, mirrored by hand from `api.h` and
//! `godot_platform.h` (bindgen-cli 0.72.1 was not available). Both sides assert sizes and offsets.
//! The leaves carry the D3D12 capture owner, the FG presenter install, header-inline Runtime
//! validators, the Vulkan owner's guard and its device, provider, presenter and SDK calls in
//! `vulkan.cpp`, and the status facts; every decision stays in this crate. The archive imports
//! no native symbol.
// Mirrored C layouts: the leaves and the engine module read fields this crate only writes.
#![allow(dead_code)]
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{c_char, c_int, c_void};
use std::sync::OnceLock;

pub(crate) const PLATFORM_VERSION: u32 = 2;
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

/// `rsf_godot_sr_frame`.
#[repr(C)]
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

/// `rsf_godot_vk_facts`, read under the Vulkan owner's guard.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkFacts {
    pub(crate) xess_device_ready: u32,
    pub(crate) interop_device_ready: u32,
    pub(crate) graph_handoff: u32,
    pub(crate) fsr4_sr_compiled: u32,
    pub(crate) fsr4_fg_compiled: u32,
    pub(crate) xess_fg_compiled: u32,
    pub(crate) presenter: u32,
    pub(crate) primary_window: u32,
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

/// `rsf_godot_vk_sr_frame`.
#[repr(C)]
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
    pub(crate) window_facts: unsafe extern "C" fn(*mut WindowFacts),
    pub(crate) fg_facts: unsafe extern "C" fn(*mut FgFacts),
    pub(crate) latency_facts: unsafe extern "C" fn(*mut LatencyFacts) -> u32,
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
    assert!(size_of::<SrFrame>() == 32);
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
    assert!(size_of::<VkSrFrame>() == 40);
    assert!(offset_of!(VkSrFrame, source_frame_id) == 32);
    assert!(size_of::<Platform>() == 184);
    assert!(offset_of!(Platform, vulkan_locked) == 88);
    assert!(offset_of!(Platform, latency_facts) == 176);
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

    pub(crate) fn window_facts(&self) -> WindowFacts {
        let mut facts = WindowFacts::default();
        unsafe { (self.window_facts)(&mut facts) };
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
}
