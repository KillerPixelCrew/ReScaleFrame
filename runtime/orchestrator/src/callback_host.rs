// SPDX-License-Identifier: MIT
//! The shared callback render host behind callback_render_host.h.
//!
//! An adapter whose render callbacks guarantee command-slot retirement starts this host with its
//! plugin, probe, saved SR choice and engine identity. Rust owns admission, the refusal codes,
//! the plugin lifecycle order, host state, renderer configuration, scene and submission
//! matching, SR context lifetime, callback recording slots, settings and overlay control.
//! callback_render_host_platform.cpp supplies one call per D3D12, DXGI, SR12, FG12, presentation
//! bridge, overlay, input dump or plugin session operation and keeps the two exported entries.
//! Settings keys belong to settings_store.rs, SR vendor selection to rsf_gpu_select_sr_backend.
//! The adapter's only policy input is `set_engine_spatial`, which learns whether the engine
//! upscales spatially itself, never the vendor.
use crate::com_ref::ComRef;
use crate::gl_pipeline_abi::{rsf_dlss_setup as DlssSetup, rsf_sr12_setup as SrSetup};
use crate::overlay_abi::{
    RSF_OVERLAY_SR_CHANGE_APPLIED, RSF_OVERLAY_SR_CHANGE_PENDING, RSF_OVERLAY_SR_CHANGE_REFUSED,
    RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK, RSF_OVERLAY_SR_QUALITY_STANDARD_MASK,
    rsf_overlay_intent as Intent, rsf_overlay_stats as Stats,
};
use crate::overlay_settings::{fill_sharpening_stats, intent_sharpening, sr_name};
use crate::render_abi::{
    RSF_DLSS_ABI_VERSION, rsf_native_fg_options as FgOptions, rsf_native_fg_status as FgStatus,
    rsf_sharpening_options as Sharpening,
};
use crate::settings_store::{
    rsf_settings_store_read_layout, rsf_settings_store_write_layout,
    rsf_settings_store_write_sharpening, write_dlss_preset,
};
use rsf_game_sys::plugin_session_abi::{
    RSF_PLUGIN_SESSION_ABI_VERSION as SESSION_ABI_VERSION,
    rsf_plugin_session_options as SessionOptions,
};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_FINAL_SCENE,
    RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR,
    RSF_GAME_RENDER_SUBMISSION, RSF_GAME_RENDER_WINDOW,
    callback_host_abi::{
        RSF_CALLBACK_HOST_ABI_VERSION as HOST_ABI_VERSION, RSF_CALLBACK_HOST_ABI_VERSION_V2,
        rsf_callback_host_setup as Setup, rsf_callback_host_setup_v2 as SetupV2,
    },
    recording_slots::RecordingSlots,
    rsf_game_cpu_event as CpuEvent, rsf_game_host_services as Services, rsf_game_probe as Probe,
    rsf_game_render_config as Config, rsf_game_render_pass as Pass,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::File;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::core::{PCWSTR, w};

#[cfg(test)]
mod tests;

type Handle = *mut c_void;
type LogFn = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;
type SpatialFn = unsafe extern "C" fn(u32, u32) -> c_int;

const OK: i32 = 0;
const BACKEND_OK: i32 = 0;
const BACKEND_NOT_SUPPORTED: i32 = -7;

const SR_NONE: u32 = 0;
const SR_DLSS: u32 = 1;
const SR_XESS: u32 = 5;
const SR_SPATIAL: u32 = 6;
const SR_AUTO: u32 = 7;
const SHARPENING_POLL_MS: u64 = 1000;
const SECTION: PCWSTR = w!("Rendering");
// FILE_APPEND_DATA, and FILE_SHARE_READ | FILE_SHARE_WRITE.
const LOG_ACCESS: u32 = 4;
const LOG_SHARE: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Adapter {
    vendor: u32,
    device: u32,
    software: u32,
}

/// The fields of a D3D12_RESOURCE_DESC the first refusal reports.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct ResourceDesc {
    width: u64,
    height: u32,
    format: u32,
    samples: u32,
}

// Field order matches `Platform` in callback_render_host_platform.cpp. Returned COM interfaces
// carry one reference for the caller; arguments are borrowed for the call. The plugin session and
// FG choice entries are the public ones, passed here so contract tests can substitute them.
#[repr(C)]
pub(crate) struct Platform {
    struct_size: u32,
    tick: unsafe extern "C" fn() -> u64,
    list_device: unsafe extern "C" fn(Handle) -> Handle,
    queue_device: unsafe extern "C" fn(Handle) -> Handle,
    adapter: unsafe extern "C" fn(Handle, *mut Adapter) -> i32,
    last_present_count: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    resource_desc: unsafe extern "C" fn(Handle, *mut ResourceDesc),
    depth_domains: unsafe extern "C" fn(*const Pass) -> u32,
    dlss_preset_valid: unsafe extern "C" fn(u32) -> u32,
    sr12_create: unsafe extern "C" fn(*const SrSetup, *mut Handle) -> i32,
    sr12_plan: unsafe extern "C" fn(Handle, *mut u32, *mut u32) -> i32,
    sr12_destroy: unsafe extern "C" fn(Handle),
    sr12_evaluate: unsafe extern "C" fn(Handle, Handle, *const Pass, u32) -> i32,
    sr12_set_sharpening: unsafe extern "C" fn(Handle, *const Sharpening) -> i32,
    fg_cpu: unsafe extern "C" fn(*const CpuEvent),
    fg_submitted: unsafe extern "C" fn(u64, u64),
    fg_hudless: unsafe extern "C" fn(Handle, *const Pass),
    fg_window: unsafe extern "C" fn(*const Pass),
    fg_status: unsafe extern "C" fn(*mut FgStatus) -> i32,
    fg_options: unsafe extern "C" fn(*const FgOptions),
    fg_choice_save: unsafe extern "C" fn(u32) -> i32,
    present_host: unsafe extern "C" fn() -> Handle,
    present_request: unsafe extern "C" fn(u32) -> i32,
    present_backend: unsafe extern "C" fn() -> u32,
    present_switch_result: unsafe extern "C" fn() -> i32,
    overlay_frame: unsafe extern "C" fn(
        Handle,
        Handle,
        Handle,
        *const Stats,
        *mut Intent,
        LogFn,
        Handle,
    ) -> i32,
    overlay_stop: unsafe extern "C" fn(),
    overlay_fill_fg: unsafe extern "C" fn(*mut Stats, *const FgStatus),
    input_dump_cancel: unsafe extern "C" fn(u64),
    session_prepare: unsafe extern "C" fn(*const SessionOptions, *mut Handle) -> i32,
    session_start: unsafe extern "C" fn(Handle) -> i32,
    session_quiesce: unsafe extern "C" fn(Handle) -> i32,
    session_stop: unsafe extern "C" fn(Handle) -> i32,
}

struct State {
    // Vendor SR context, and the backend-zero context that normalizes generation inputs.
    sr: Handle,
    inputs: Handle,
    input_width: u32,
    input_height: u32,
    plan: Config,
    // Recording slots, bound to the first three command lists the adapter records with.
    command_lists: RecordingSlots,
    // `requested_*` is what the panel or start chose; `backend` and `quality` are what the
    // current context was prepared with.
    backend: u32,
    quality: u32,
    requested_backend: u32,
    requested_quality: u32,
    sr_change_id: u32,
    sr_change_status: u32,
    preset: u32,
    sharpening: Sharpening,
    sharpening_poll: u64,
    dirty: bool,
    requested_generation: Option<u32>,
    enabled: bool,
    show_performance: bool,
    loaded_frame_limit: bool,
    // Panel layout word, saved in the preferences file under its own key.
    ui_preferences: u32,
    automatic: bool,
    last_result: i32,
    overlay_frames: u64,
    width: u32,
    height: u32,
    attempted: bool,
    accepted: u64,
    refused: u64,
    // The chosen generation provider is saved for the next start only once the switch has landed.
    save_pending: bool,
    save_backend: u32,
}

pub(crate) struct Host {
    p: &'static Platform,
    state: Mutex<State>,
    log_file: Mutex<File>,
    // Written only under the lifecycle lock; read by stop.
    plugin: AtomicPtr<c_void>,
    preferences: Vec<u16>,
    streamline: CString,
    interposer: CString,
    fsr2: CString,
    fsr3: CString,
    fsr4: CString,
    xess: CString,
    sr_choices: u32,
    engine_type: u32,
    engine_version: CString,
    project_id: CString,
    set_engine_spatial: SpatialFn,
    policy_v2: Option<PolicyV2>,
}

struct PolicyV2 {
    request: unsafe extern "C" fn(u32, u32, *mut u32) -> c_int,
    drained: unsafe extern "C" fn(*mut u32) -> c_int,
    extent: unsafe extern "C" fn(u32, u32, u32, *mut u32, *mut u32) -> c_int,
    spatial_name: CString,
}

fn quality_choices(backend: u32) -> u32 {
    match backend {
        SR_DLSS => RSF_OVERLAY_SR_QUALITY_STANDARD_MASK,
        0..=SR_SPATIAL => RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK,
        _ => 0,
    }
}

fn accepts_backend_quality(backend: u32, quality: u32) -> bool {
    quality < 32 && quality_choices(backend) & (1 << quality) != 0
}

struct Published(*mut Host);
// The host is shared with callbacks through its address; this only moves that address.
unsafe impl Send for Published {}

// The one started host. Start and stop serialize on this lock.
static LIFECYCLE: Mutex<Published> = Mutex::new(Published(null_mut()));

fn try_lifecycle() -> Option<MutexGuard<'static, Published>> {
    // Adapter callbacks may reenter either public lifecycle entry.
    match LIFECYCLE.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

unsafe fn wide_copy(text: *const u16) -> Vec<u16> {
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    unsafe { std::slice::from_raw_parts(text, length + 1) }.to_vec()
}

unsafe fn c_copy(text: *const c_char) -> CString {
    if text.is_null() {
        CString::default()
    } else {
        unsafe { CStr::from_ptr(text) }.to_owned()
    }
}

fn host_from<'a>(user: *mut c_void) -> &'a Host {
    unsafe { &*user.cast::<Host>() }
}

fn empty_plan() -> Config {
    Config {
        struct_size: 0,
        enabled: 0,
        output_width: 0,
        output_height: 0,
        render_width: 0,
        render_height: 0,
    }
}

impl Host {
    fn user(&self) -> Handle {
        ptr::from_ref(self).cast_mut().cast()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn log_bytes(&self, message: &[u8]) {
        let mut file = self.log_file.lock().unwrap_or_else(PoisonError::into_inner);
        let mut line = Vec::with_capacity(message.len() + 2);
        line.extend_from_slice(message);
        line.extend_from_slice(b"\r\n");
        let _ = file.write_all(&line);
    }

    fn log(&self, message: &str) {
        self.log_bytes(message.as_bytes());
    }

    // Whether the adapter accepts `backend` at `quality`. It learns only whether the engine
    // upscales spatially itself.
    fn policy(&self, backend: u32, quality: u32) -> bool {
        unsafe { (self.set_engine_spatial)(u32::from(backend == SR_SPATIAL), quality) != 0 }
    }

    fn request_reconfigure(&self, state: &mut State, backend: u32, quality: u32) -> bool {
        if let Some(adapter) = &self.policy_v2 {
            let mut generation = 0;
            if unsafe {
                (adapter.request)(u32::from(backend == SR_SPATIAL), quality, &mut generation)
            } == 0
            {
                return false;
            }
            state.requested_generation = Some(generation);
        } else if !self.policy(backend, quality) {
            return false;
        }
        state.dirty = true;
        state.plan.enabled = 0;
        true
    }

    // The shared post-pass keys of the preferences section, sanitized.
    fn read_sharpening(&self) -> Sharpening {
        unsafe { crate::settings_store::read_sharpening(self.preferences.as_ptr(), SECTION) }
    }

    fn destroy(&self, context: &mut Handle) {
        if !context.is_null() {
            unsafe { (self.p.sr12_destroy)(*context) };
            *context = null_mut();
        }
    }

    fn release_rendering(&self, stop_overlay: bool) {
        if stop_overlay {
            unsafe { (self.p.overlay_stop)() };
        }
        let mut state = self.lock();
        self.destroy(&mut state.sr);
        self.destroy(&mut state.inputs);
    }

    fn render(&self, commands: Handle, pass: &Pass, begin: bool) {
        let p = self.p;
        if unsafe { (p.depth_domains)(pass) } != 0 {
            return;
        }
        if !begin {
            if matches!(
                pass.role,
                RSF_GAME_RENDER_SR
                    | RSF_GAME_RENDER_FG_INPUTS
                    | RSF_GAME_RENDER_FINAL_SCENE
                    | RSF_GAME_RENDER_SUBMISSION
            ) {
                unsafe { (p.fg_submitted)(pass.session_id, pass.source_frame_id) };
            }
            return;
        }
        match pass.role {
            RSF_GAME_RENDER_FINAL_SCENE => unsafe { (p.fg_hudless)(commands, pass) },
            RSF_GAME_RENDER_WINDOW => self.window(commands, pass),
            RSF_GAME_RENDER_SR | RSF_GAME_RENDER_FG_INPUTS => {
                let mut state = self.lock();
                self.scene(&mut state, commands, pass);
            }
            _ => {}
        }
    }

    // Window pass: provider switch bookkeeping, the shared panel and its requested changes.
    fn window(&self, queue: Handle, pass: &Pass) {
        let p = self.p;
        let mut guard = self.lock();
        let s = &mut *guard;
        unsafe { (p.fg_window)(pass) };
        let Some(device) = (unsafe { ComRef::from_owned((p.queue_device)(queue)) }) else {
            return;
        };
        if s.save_pending {
            if unsafe { (p.present_switch_result)() } != 0 {
                s.save_pending = false;
                self.log("Generation provider switch failed; the saved choice is unchanged.");
            } else if unsafe { (p.present_backend)() } == s.save_backend {
                s.save_pending = false;
                let _ = unsafe { (p.fg_choice_save)(s.save_backend) };
            }
        }
        let mut stats = Stats {
            struct_size: size_of::<Stats>() as u32,
            backend_loaded: 1,
            backend_supported: s.plan.enabled,
            backend_name: self
                .policy_v2
                .as_ref()
                .filter(|adapter| s.backend == SR_SPATIAL && !adapter.spatial_name.is_empty())
                .map_or_else(
                    || sr_name(s.backend).as_ptr(),
                    |adapter| adapter.spatial_name.as_ptr(),
                ),
            backend: s.backend,
            sr_backend_choices: self.sr_choices,
            sr_quality_choices: quality_choices(s.backend),
            sr_change_id: s.sr_change_id,
            sr_change_status: s.sr_change_status,
            requested_backend: s.requested_backend,
            quality: s.quality,
            sharpening_available: u32::from((SR_DLSS..=SR_XESS).contains(&s.backend)),
            dlss_preset: s.preset,
            dlss_preset_available: 1,
            enabled: u32::from(s.enabled),
            last_result: s.last_result,
            last_switch_result: s.last_result,
            render_width: s.plan.render_width,
            render_height: s.plan.render_height,
            output_width: pass.camera.output_width,
            output_height: pass.camera.output_height,
            frames_evaluated: s.accepted as u32,
            frames_refused: s.refused as u32,
            reinsert_available: 1,
            reinsert_on: s.plan.enabled,
            have_scene_color: u32::from(s.accepted != 0),
            have_depth: u32::from(s.accepted != 0),
            have_motion: u32::from(s.accepted != 0),
            motion_decoded: u32::from(s.accepted != 0),
            jitter_available: 1,
            jitter_active: u32::from(s.plan.enabled != 0 && s.backend != SR_SPATIAL),
            render_scale_percent: s
                .plan
                .render_width
                .wrapping_mul(100)
                .checked_div(s.width)
                .unwrap_or(0),
            ..Stats::default()
        };
        fill_sharpening_stats(&mut stats, &s.sharpening);
        let reason: Option<&'static CStr> = if s.dirty {
            Some(c"Applying requested configuration after GPU completion")
        } else if s.last_result != 0 {
            Some(c"Provider refused; spatial fallback retained")
        } else if s.accepted == 0 && s.backend != SR_SPATIAL {
            Some(c"Prepared; awaiting an eligible scene evaluation")
        } else if s.backend == SR_SPATIAL {
            Some(c"Sharpening unavailable with engine spatial upscaling")
        } else {
            None
        };
        stats.refusal_reason = reason.map_or(null(), CStr::as_ptr);
        s.overlay_frames += 1;
        stats.frames_presented = s.overlay_frames as u32;
        let mut presented = 0u32;
        if unsafe { (p.last_present_count)(pass.swapchain, &mut presented) } != 0 {
            stats.application_presented_frames = u64::from(presented);
        }
        let (mut now, mut frequency) = (0i64, 0i64);
        unsafe {
            QueryPerformanceCounter(&mut now);
            QueryPerformanceFrequency(&mut frequency);
        }
        stats.sample_qpc = now as u64;
        stats.qpc_frequency = frequency as u64;
        let mut fg = FgStatus {
            struct_size: size_of::<FgStatus>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let have_fg = unsafe { (p.fg_status)(&mut fg) } != 0;
        if have_fg && !s.loaded_frame_limit {
            s.loaded_frame_limit = true;
            let limit = unsafe {
                crate::settings_store::rsf_settings_store_read_integer(
                    self.preferences.as_ptr(),
                    SECTION,
                    c"FrameLimitUs".as_ptr(),
                    fg.requested.frame_limit_us,
                )
            };
            if limit != fg.requested.frame_limit_us {
                fg.requested.frame_limit_us = limit;
                unsafe { (p.fg_options)(&fg.requested) };
            }
        }
        unsafe { (p.overlay_fill_fg)(&mut stats, if have_fg { &fg } else { null() }) };
        // The saved choice stays on the old provider until the switch lands, so show the target.
        if s.save_pending {
            stats.fg_requested_backend = s.save_backend;
        }
        stats.show_performance_hud = u32::from(s.show_performance);
        stats.ui_preferences = s.ui_preferences;
        let saved = crate::overlay_saved::Frame::new(self.user());
        saved.attach(&mut stats);
        let mut intent = Intent {
            struct_size: size_of::<Intent>() as u32,
            ..Intent::default()
        };
        // The panel draw and its GPU interop need no host state, and the renderer configuration
        // query waits on this lock.
        drop(guard);
        let drawn = unsafe {
            (p.overlay_frame)(
                device.as_ptr(),
                queue,
                pass.swapchain,
                &stats,
                &mut intent,
                Some(log),
                self.user(),
            )
        };
        if drawn != 0 {
            saved.apply(&intent);
            let mut guard = self.lock();
            self.apply_intent(&mut guard, &intent, &mut fg.requested);
        }
    }

    fn apply_intent(&self, s: &mut State, intent: &Intent, fg: &mut FgOptions) {
        let p = self.p;
        if intent.fg_backend_changed != 0 {
            let applied = unsafe { (p.present_request)(intent.fg_backend) };
            if applied == BACKEND_OK {
                s.save_pending = true;
                s.save_backend = intent.fg_backend;
            }
            self.log(if applied == BACKEND_OK {
                "Generation provider requested at the next drained Present."
            } else {
                "Generation provider request refused."
            });
        }
        if intent.fg_changed != 0 || intent.reflex_changed != 0 || intent.frame_limit_changed != 0 {
            if intent.fg_changed != 0 {
                fg.mode = intent.fg_mode;
                fg.generated_frames = intent.fg_generated;
            }
            if intent.reflex_changed != 0 {
                fg.reflex_mode = intent.reflex_mode;
            }
            if intent.frame_limit_changed != 0 {
                let saved = unsafe {
                    crate::settings_store::rsf_settings_store_write_integer(
                        self.preferences.as_ptr(),
                        SECTION,
                        c"FrameLimitUs".as_ptr(),
                        intent.frame_limit_us,
                    )
                };
                if saved == OK {
                    fg.frame_limit_us = intent.frame_limit_us;
                } else {
                    self.log("Could not save the frame limit; keeping the active limit.");
                }
            }
            unsafe { (p.fg_options)(fg) };
        }
        if intent.performance_hud_changed != 0 {
            s.show_performance = intent.performance_hud != 0;
        }
        if intent.ui_preferences_changed != 0 && intent.ui_preferences != s.ui_preferences {
            s.ui_preferences = intent.ui_preferences;
            let saved = unsafe {
                rsf_settings_store_write_layout(
                    self.preferences.as_ptr(),
                    SECTION,
                    intent.ui_preferences,
                )
            };
            if saved != OK {
                self.log("Could not save the panel layout; it applies for this session.");
            }
        }
        if intent.sharpening_changed != 0 {
            let requested = intent_sharpening(intent);
            if unsafe {
                rsf_settings_store_write_sharpening(self.preferences.as_ptr(), SECTION, &requested)
            } == OK
            {
                s.sharpening = requested;
                if !s.sr.is_null() {
                    let _ = unsafe { (p.sr12_set_sharpening)(s.sr, &s.sharpening) };
                }
            } else {
                self.log("Could not save sharpening settings; previous values retained.");
            }
        }
        let preset_changed = intent.dlss_preset_changed != 0
            && intent.dlss_preset != s.preset
            && unsafe { (p.dlss_preset_valid)(intent.dlss_preset) } != 0;
        let policy_changed = intent.backend_changed != 0
            || intent.quality_changed != 0
            || intent.enabled_changed != 0
            || intent.start_requested != 0;
        if !preset_changed && !policy_changed {
            return;
        }
        let sr_edit = intent.sr_change_id != 0
            && (intent.quality_changed != 0 || intent.enabled_changed != 0);
        if sr_edit {
            s.sr_change_id = intent.sr_change_id;
            s.sr_change_status = RSF_OVERLAY_SR_CHANGE_REFUSED;
        }
        let backend = if intent.backend_changed != 0 {
            intent.backend
        } else {
            s.requested_backend
        };
        let quality = if intent.quality_changed != 0 {
            intent.quality
        } else {
            s.requested_quality
        };
        if !accepts_backend_quality(backend, quality)
            || !self.request_reconfigure(s, backend, quality)
        {
            return;
        }
        s.requested_backend = backend;
        s.requested_quality = quality;
        if sr_edit {
            s.sr_change_status = RSF_OVERLAY_SR_CHANGE_PENDING;
        }
        if intent.backend_changed != 0 {
            s.automatic = false;
        }
        if intent.enabled_changed != 0 {
            s.enabled = intent.enabled != 0;
        }
        if preset_changed {
            s.preset = intent.dlss_preset;
            if unsafe { write_dlss_preset(self.preferences.as_ptr(), SECTION, s.preset) } != OK {
                self.log("Could not save requested DLSS model preset.");
            }
            self.log("DLSS model change requested; draining previous GPU work.");
        } else {
            self.log("Overlay requested vendor/quality change; draining previous GPU work.");
        }
    }

    // SR and generation-input passes on the adapter's recording thread.
    fn scene(&self, s: &mut State, commands: Handle, pass: &Pass) {
        let p = self.p;
        let camera = &pass.camera;
        let mut drained = pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0;
        if let (Some(adapter), Some(expected)) = (&self.policy_v2, s.requested_generation) {
            let mut generation = 0;
            drained = drained
                && unsafe { (adapter.drained)(&mut generation) } != 0
                && generation == expected;
            if !drained {
                return;
            }
        }
        if (s.dirty && drained)
            || s.width != camera.output_width
            || s.height != camera.output_height
        {
            // The adapter's submission path joins its previous frame-fence values before this.
            self.destroy(&mut s.sr);
            self.destroy(&mut s.inputs);
            s.input_width = 0;
            s.input_height = 0;
            s.width = camera.output_width;
            s.height = camera.output_height;
            s.attempted = false;
            s.plan = empty_plan();
            s.backend = s.requested_backend;
            s.quality = s.requested_quality;
            s.dirty = false;
            s.requested_generation = None;
            s.accepted = 0;
            s.refused = 0;
            s.last_result = 0;
        }
        if !s.attempted {
            let prepared = self.prepare(s, commands, pass);
            if !s.dirty && s.sr_change_status == RSF_OVERLAY_SR_CHANGE_PENDING {
                s.sr_change_status = if prepared || s.last_result == BACKEND_OK {
                    RSF_OVERLAY_SR_CHANGE_APPLIED
                } else {
                    RSF_OVERLAY_SR_CHANGE_REFUSED
                };
            }
            if !prepared {
                return;
            }
        }
        if pass.role == RSF_GAME_RENDER_FG_INPUTS && pass.camera_valid != 0 {
            if s.inputs.is_null()
                || s.input_width != camera.render_width
                || s.input_height != camera.render_height
            {
                self.destroy(&mut s.inputs);
                let Some(device) = (unsafe { ComRef::from_owned((p.list_device)(commands)) })
                else {
                    return;
                };
                let setup = SrSetup {
                    struct_size: size_of::<SrSetup>() as u32,
                    abi_version: 1,
                    device: device.as_ptr(),
                    output_width: camera.output_width,
                    output_height: camera.output_height,
                    render_width: camera.render_width,
                    render_height: camera.render_height,
                    log: Some(log),
                    user: self.user(),
                    ..SrSetup::default()
                };
                if unsafe { (p.sr12_create)(&setup, &mut s.inputs) } != BACKEND_OK {
                    return;
                }
                s.input_width = setup.render_width;
                s.input_height = setup.render_height;
            }
            if let Some(slot) = s.command_lists.bind(commands) {
                let _ = unsafe { (p.sr12_evaluate)(s.inputs, commands, pass, slot) };
            }
            return;
        }
        if s.sr.is_null() || pass.camera_valid == 0 {
            return;
        }
        let Some(slot) = s.command_lists.bind(commands) else {
            return;
        };
        let now = unsafe { (p.tick)() };
        if now >= s.sharpening_poll {
            s.sharpening_poll = now.saturating_add(SHARPENING_POLL_MS);
            s.sharpening = self.read_sharpening();
            let _ = unsafe { (p.sr12_set_sharpening)(s.sr, &s.sharpening) };
        }
        let result = unsafe { (p.sr12_evaluate)(s.sr, commands, pass, slot) };
        if result != BACKEND_OK && s.refused == 0 {
            let resources = [pass.color_input, pass.depth, pass.motion, pass.color_output];
            for (index, &resource) in resources.iter().enumerate() {
                if resource.is_null() {
                    continue;
                }
                let mut desc = ResourceDesc::default();
                unsafe { (p.resource_desc)(resource, &mut desc) };
                self.log(&format!(
                    "SR input {index}: {}x{} format={} samples={}; camera={}x{} plan={}x{}",
                    desc.width,
                    desc.height,
                    desc.format,
                    desc.samples,
                    camera.render_width,
                    camera.render_height,
                    s.plan.render_width,
                    s.plan.render_height
                ));
            }
        }
        s.last_result = result;
        if result == BACKEND_OK {
            s.accepted += 1;
        } else {
            s.refused += 1;
        }
        let report = if result == BACKEND_OK {
            s.accepted <= 3 || s.accepted.is_multiple_of(600)
        } else {
            s.refused <= 3
        };
        if report {
            self.log(&format!(
                "Native renderer SR view {} frame {} result {result}, accepted={} refused={} reset={} jitter={:.3},{:.3}",
                pass.view_key,
                pass.native_frame,
                s.accepted,
                s.refused,
                pass.flags & RSF_GAME_RENDER_RESET,
                camera.jitter_pixels[0],
                camera.jitter_pixels[1]
            ));
        }
    }

    // The first SR pass after start or reconfiguration: resolve Auto, then plan the engine's
    // spatial path or prepare the vendor context. False ends this pass.
    fn prepare(&self, s: &mut State, commands: Handle, pass: &Pass) -> bool {
        let p = self.p;
        s.attempted = true;
        s.plan = Config {
            struct_size: size_of::<Config>() as u32,
            enabled: 0,
            output_width: s.width,
            output_height: s.height,
            render_width: 0,
            render_height: 0,
        };
        if s.automatic {
            let mut adapter = Adapter::default();
            let identified = unsafe { ComRef::from_owned((p.list_device)(commands)) }
                .is_some_and(|device| unsafe { (p.adapter)(device.as_ptr(), &mut adapter) } != 0);
            if !identified {
                s.last_result = BACKEND_NOT_SUPPORTED;
                self.log("Automatic SR refused: owning D3D12 adapter could not be identified.");
                return false;
            }
            s.backend =
                crate::rsf_gpu_select_sr_backend(adapter.vendor, adapter.software, self.sr_choices);
            s.requested_backend = s.backend;
            if !accepts_backend_quality(s.backend, s.quality)
                || (self.policy_v2.is_none() && !self.policy(s.backend, s.quality))
            {
                return false;
            }
            self.log(&format!(
                "Auto SR selected backend {} from owning D3D12 adapter vendor={:#x} device={:#x}",
                s.backend, adapter.vendor, adapter.device
            ));
        }
        if s.backend == SR_NONE || !s.enabled {
            // Native rendering still supplies generation inputs.
            return true;
        }
        if s.backend == SR_SPATIAL {
            let (mut width, mut height) = (pass.camera.render_width, pass.camera.render_height);
            if let Some(adapter) = &self.policy_v2
                && unsafe {
                    (adapter.extent)(s.quality, s.width, s.height, &mut width, &mut height)
                } == 0
            {
                s.last_result = BACKEND_NOT_SUPPORTED;
                return false;
            }
            if width == 0 || height == 0 || width > s.width || height > s.height {
                s.last_result = BACKEND_NOT_SUPPORTED;
                return false;
            }
            s.plan.render_width = width;
            s.plan.render_height = height;
            s.plan.enabled = 1;
            self.log("Engine spatial upscaling plan accepted.");
            return true;
        }
        let Some(device) = (unsafe { ComRef::from_owned((p.list_device)(commands)) }) else {
            return false;
        };
        let setup = SrSetup {
            struct_size: size_of::<SrSetup>() as u32,
            abi_version: 1,
            device: device.as_ptr(),
            backend: s.backend,
            quality: s.quality,
            streamline_host: unsafe { (p.present_host)() },
            output_width: s.width,
            output_height: s.height,
            inverted_depth: pass.camera.depth_inverted,
            fsr2_directory_utf8: self.fsr2.as_ptr(),
            fsr3_directory_utf8: self.fsr3.as_ptr(),
            fsr4_directory_utf8: self.fsr4.as_ptr(),
            xess_directory_utf8: self.xess.as_ptr(),
            log: Some(log),
            user: self.user(),
            dlss: DlssSetup {
                struct_size: size_of::<DlssSetup>() as u32,
                abi_version: RSF_DLSS_ABI_VERSION,
                interposer_path_utf8: self.interposer.as_ptr(),
                plugin_directory_utf8: self.streamline.as_ptr(),
                engine: self.engine_type,
                engine_version_utf8: self.engine_version.as_ptr(),
                project_id_utf8: self.project_id.as_ptr(),
                preset: s.preset,
                require_signature: 1,
                log: Some(log),
                log_user: self.user(),
                ..DlssSetup::default()
            },
            ..SrSetup::default()
        };
        let created = unsafe { (p.sr12_create)(&setup, &mut s.sr) };
        drop(device);
        s.last_result = created;
        if created == BACKEND_OK {
            let now = unsafe { (p.tick)() };
            if now >= s.sharpening_poll {
                s.sharpening = self.read_sharpening();
                s.sharpening_poll = now.saturating_add(SHARPENING_POLL_MS);
            }
            let _ = unsafe { (p.sr12_set_sharpening)(s.sr, &s.sharpening) };
            let _ =
                unsafe { (p.sr12_plan)(s.sr, &mut s.plan.render_width, &mut s.plan.render_height) };
            s.plan.enabled = 1;
        }
        self.log(&format!(
            "Native D3D12 provider {} preparation result {created}, {}x{} -> {}x{}",
            s.backend, s.plan.render_width, s.plan.render_height, s.width, s.height
        ));
        true
    }
}

// Service callbacks. `owner` is the host published by start.

unsafe extern "C" fn log(owner: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        host_from(owner).log_bytes(unsafe { CStr::from_ptr(message) }.to_bytes());
    }
}

unsafe extern "C" fn configuration(owner: *mut c_void, output: *mut Config) -> c_int {
    if output.is_null() || (unsafe { output.cast::<u32>().read() } as usize) < size_of::<Config>() {
        return 0;
    }
    let plan = host_from(owner).lock().plan;
    unsafe {
        output.write(Config {
            struct_size: size_of::<Config>() as u32,
            ..plan
        })
    };
    1
}

unsafe extern "C" fn render(owner: *mut c_void, commands: Handle, pass: *const Pass, begin: u32) {
    if commands.is_null()
        || pass.is_null()
        || (unsafe { pass.cast::<u32>().read() } as usize) < size_of::<Pass>()
    {
        return;
    }
    host_from(owner).render(commands, unsafe { &*pass }, begin != 0);
}

unsafe extern "C" fn cpu_event(owner: *mut c_void, event: *const CpuEvent) {
    unsafe { (host_from(owner).p.fg_cpu)(event) };
}

// Admission and host construction, before any plugin is loaded. Err is the public refusal code:
// 1 incomplete or mismatched setup, 2 invalid choice, 4 log unavailable, 8 adapter policy refused.
unsafe fn describe(setup: *const Setup, p: &'static Platform) -> Result<Host, u32> {
    if setup.is_null() || (unsafe { setup.cast::<u32>().read() } as usize) < size_of::<Setup>() {
        return Err(1);
    }
    let setup_ptr = setup;
    let setup = unsafe { &*setup };
    let Some(set_engine_spatial) = setup.set_engine_spatial else {
        return Err(1);
    };
    let policy_v2 = match setup.abi_version {
        HOST_ABI_VERSION => None,
        RSF_CALLBACK_HOST_ABI_VERSION_V2 if setup.struct_size as usize >= size_of::<SetupV2>() => {
            let extended = unsafe { &*setup_ptr.cast::<SetupV2>() };
            Some(PolicyV2 {
                request: extended.request_policy.ok_or(1u32)?,
                drained: extended.drained_generation.ok_or(1u32)?,
                extent: extended.spatial_extent.ok_or(1u32)?,
                spatial_name: unsafe { c_copy(extended.spatial_name_utf8) },
            })
        }
        _ => return Err(1),
    };
    if setup.plugin_path.is_null()
        || setup.log_path.is_null()
        || setup.preferences_path.is_null()
        || setup.probe.is_null()
        || setup.engine_version_utf8.is_null()
        || setup.project_id_utf8.is_null()
        || setup.sr_backend_choices == 0
    {
        return Err(1);
    }
    let automatic = setup.backend == SR_AUTO;
    let backend = if automatic { SR_NONE } else { setup.backend };
    let quality = setup.quality;
    if unsafe { (p.dlss_preset_valid)(setup.dlss_preset) } == 0
        || !accepts_backend_quality(backend, quality)
    {
        return Err(2);
    }
    let log_path = unsafe { wide_copy(setup.log_path) };
    let log_file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .access_mode(LOG_ACCESS)
        .share_mode(LOG_SHARE)
        .open(std::ffi::OsString::from_wide(
            &log_path[..log_path.len() - 1],
        ))
        .map_err(|_| 4u32)?;
    let streamline = unsafe { c_copy(setup.streamline_directory_utf8) };
    let mut interposer = streamline.as_bytes().to_vec();
    interposer.extend_from_slice(b"\\sl.interposer.dll");
    let preferences = unsafe { wide_copy(setup.preferences_path) };
    let ui_preferences = unsafe { rsf_settings_store_read_layout(preferences.as_ptr(), SECTION) };
    let host = Host {
        p,
        state: Mutex::new(State {
            sr: null_mut(),
            inputs: null_mut(),
            input_width: 0,
            input_height: 0,
            plan: empty_plan(),
            command_lists: RecordingSlots::EMPTY,
            backend,
            quality,
            requested_backend: backend,
            requested_quality: quality,
            sr_change_id: 0,
            sr_change_status: 0,
            preset: setup.dlss_preset,
            sharpening: unsafe { std::mem::zeroed() },
            sharpening_poll: 0,
            dirty: false,
            requested_generation: None,
            enabled: true,
            show_performance: false,
            loaded_frame_limit: false,
            ui_preferences,
            automatic,
            last_result: 0,
            overlay_frames: 0,
            width: 0,
            height: 0,
            attempted: false,
            accepted: 0,
            refused: 0,
            save_pending: false,
            save_backend: 0,
        }),
        log_file: Mutex::new(log_file),
        plugin: AtomicPtr::new(null_mut()),
        preferences,
        streamline,
        interposer: CString::new(interposer).unwrap_or_default(),
        fsr2: unsafe { c_copy(setup.fsr2_directory_utf8) },
        fsr3: unsafe { c_copy(setup.fsr3_directory_utf8) },
        fsr4: unsafe { c_copy(setup.fsr4_directory_utf8) },
        xess: unsafe { c_copy(setup.xess_directory_utf8) },
        sr_choices: setup.sr_backend_choices,
        engine_type: setup.engine_type,
        engine_version: unsafe { c_copy(setup.engine_version_utf8) },
        project_id: unsafe { c_copy(setup.project_id_utf8) },
        set_engine_spatial,
        policy_v2,
    };
    let sharpening = host.read_sharpening();
    host.lock().sharpening = sharpening;
    if !host.policy(backend, quality) {
        return Err(8);
    }
    Ok(host)
}

fn retire_plugin(platform: &Platform, plugin: Handle) -> bool {
    plugin.is_null()
        || (unsafe { (platform.session_quiesce)(plugin) } == OK
            && unsafe { (platform.session_stop)(plugin) } == OK)
}

// Refusal codes beyond describe: 9 a refused plugin could not be stopped and the host stays
// published to retry, 10 the plugin refused preparation or activation, 11 no host allocation.
unsafe fn start(setup: *const Setup, p: &'static Platform) -> u32 {
    let Some(mut published) = try_lifecycle() else {
        return 1;
    };
    if !published.0.is_null() {
        return 1;
    }
    let host = match unsafe { describe(setup, p) } {
        Ok(host) => host,
        Err(code) => return code,
    };
    let setup = unsafe { &*setup };
    // The plugin may keep the services' user address until its session stops.
    let raw = crate::allocate_handle(host);
    if raw.is_null() {
        return 11;
    }
    let host = unsafe { &*raw };
    let options = SessionOptions {
        struct_size: size_of::<SessionOptions>() as u32,
        abi_version: SESSION_ABI_VERSION,
        plugin_path: setup.plugin_path,
        probe: setup.probe,
        services: Services {
            struct_size: size_of::<Services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: unsafe { (p.tick)() }.wrapping_add(1),
            log: Some(log),
            render_pass: Some(render),
            render_config: Some(configuration),
            user: raw.cast(),
            cpu_event: Some(cpu_event),
            ..unsafe { std::mem::zeroed() }
        },
    };
    let mut plugin = null_mut();
    let mut result = unsafe { (p.session_prepare)(&options, &mut plugin) };
    if result == OK {
        result = unsafe { (p.session_start)(plugin) };
    }
    host.plugin.store(plugin, Ordering::Release);
    if result != OK {
        host.log("Native renderer plugin preparation/activation refused.");
        if !retire_plugin(p, plugin) {
            published.0 = raw;
            return 9;
        }
        // Callbacks can have created contexts or given the panel our log
        // address before start refused. Keep that address until cleanup ends.
        let used_overlay = host.lock().overlay_frames != 0;
        host.release_rendering(used_overlay);
        drop(unsafe { Box::from_raw(raw) });
        return 10;
    }
    host.log("Adapter lifecycle activated; awaiting actual D3D12 render callback.");
    published.0 = raw;
    0
}

// 1 while the plugin refuses quiesce or stop; the host stays published to retry.
fn stop() -> u32 {
    let Some(mut published) = try_lifecycle() else {
        return 1;
    };
    let raw = published.0;
    if raw.is_null() {
        return 0;
    }
    let host = unsafe { &*raw };
    let p = host.p;
    let plugin = host.plugin.load(Ordering::Acquire);
    if !retire_plugin(p, plugin) {
        return 1;
    }
    host.plugin.store(null_mut(), Ordering::Release);
    host.release_rendering(true);
    unsafe { (p.input_dump_cancel)(0) };
    host.log("Native renderer SR stopped after managed/native/GPU retirement.");
    published.0 = null_mut();
    drop(unsafe { Box::from_raw(raw) });
    0
}

#[cfg(not(test))]
unsafe extern "C" {
    fn rsf_callback_host_platform() -> &'static Platform;
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_callback_host_start(setup: *const Setup) -> u32 {
    unsafe { start(setup, rsf_callback_host_platform()) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_callback_host_stop() -> u32 {
    stop()
}
