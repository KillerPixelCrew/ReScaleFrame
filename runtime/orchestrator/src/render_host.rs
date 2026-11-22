// SPDX-License-Identifier: MIT
//! The generic D3D12 render host behind render_host.h and render_frontend.h.
//!
//! Rust owns admission, host and control state, SR backend resolution, settings translation and
//! commit, SR context lifetime, recording slots, fence retirement, scene and Present matching,
//! the frontend status and overlay routing. render_host_platform.cpp supplies one call per
//! D3D12, DXGI, SR12, DLSS registration, FG12, presentation bridge, overlay or observer operation
//! and keeps the exported entry points. Settings keys and text belong to settings_store.rs,
//! SR vendor selection to rsf_gpu_select_sr_backend and FG provider admission to fg_choice.rs.
use crate::com_ref::ComRef;
use crate::d3d12_observer::ObserverSetup;
use crate::fg_choice;
use crate::fg12_abi::rsf_fg12_setup as FgSetup;
use crate::gl_pipeline_abi::{rsf_dlss_setup as DlssSetup, rsf_sr12_setup as SrSetup};
use crate::overlay_abi::{
    RSF_NATIVE_FG_REASON_HUDLESS_MISSING, RSF_NATIVE_FG_REASON_RESET,
    RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED, RSF_OVERLAY_FG_RUNTIME_SWITCH, RSF_REFLEX_ON,
    rsf_overlay_intent as Intent, rsf_overlay_stats as Stats, rsf_render_status as Status,
};
use crate::overlay_settings::{
    apply_settings_intent, fill_sharpening_stats, settings_changed, sr_name,
};
use crate::present_rates::Rates;
use crate::render_abi::{
    RSF_DLSS_ABI_VERSION, RSF_FG_BACKEND_ALL, RSF_FG_BACKEND_AUTO, RSF_FG_FIXED, RSF_FG_OFF,
    RSF_FG_STAT_TOTAL_PRESENTED, RSF_RENDER_FRONTEND_ABI_VERSION,
    rsf_native_fg_options as FgOptions, rsf_native_fg_status as FgStatus,
    rsf_render_settings as Settings, rsf_sharpening_options as Sharpening,
};
use crate::settings_store::{
    read_settings_and_fg_mode, rsf_settings_store_read_layout, rsf_settings_store_write,
    rsf_settings_store_write_layout, set_sharpening, sharpening_of,
};
use crate::sharpening_policy;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RESET,
    RSF_GAME_RENDER_WINDOW, rsf_game_cpu_event as CpuEvent, rsf_game_host_services as Services,
    rsf_game_latency_event as LatencyEvent, rsf_game_log_fn, rsf_game_render_feedback as Feedback,
    rsf_game_render_pass as Pass,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW, WritePrivateProfileStringW,
};
use windows_sys::core::w;

#[cfg(test)]
mod tests;

type Handle = *mut c_void;
type LogFn = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;
type WindowFn = Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;
const NATIVE_REFUSED: i32 = -5;
const BACKEND_OK: i32 = 0;
const BACKEND_NOT_READY: i32 = -8;

const HOST_ABI_VERSION: u32 = 1;
const ENGINE_PRESENTATION: u32 = 1;
const SR_NONE: u32 = 0;
const SR_DLSS: u32 = 1;
const SR_XESS: u32 = 5;
const SR_AUTO: u32 = 7;
const PRESENT_TEST: u32 = 1;
const SLOTS: usize = 3;
const SCENES: usize = 64;
const CAPTURE_PATH: usize = 32768;
const FENCE_TIMEOUT_MS: u32 = 15000;
// Present-ratio quality index: native, quality, balanced, performance, ultra performance.
const QUALITY_RATIOS: [f32; 5] = [1.0, 1.5, 1.72, 2.0, 3.0];
const RENDERER_REASONS: [&str; 8] = [
    "",
    "Unsupported game inputs",
    "Unsupported input rectangle",
    "Camera unavailable",
    "Unsupported output",
    "Invalid camera or frozen jitter",
    "Presentation queue unavailable",
    "SR backend refused frame",
];

// Values for keys missing from the section, unchanged from the original frontend reads.
const DEFAULTS: Settings = Settings {
    struct_size: size_of::<Settings>() as u32,
    abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
    sr_enabled: 1,
    sr_backend: SR_AUTO,
    dlss_preset: 0,
    fg_enabled: 0,
    fg_backend: RSF_FG_BACKEND_AUTO,
    generated_frames: 1,
    latency: RSF_REFLEX_ON,
    frame_limit_us: 0,
    status_overlay: 1,
    sr_quality: 2,
    ..unsafe { std::mem::zeroed() }
};

/// render_host.h's rsf_render_host_setup; render_host_platform.cpp asserts the layout.
#[repr(C)]
pub(crate) struct Setup {
    struct_size: u32,
    abi_version: u32,
    configuration_path: *const u16,
    runtime_directory: *const u16,
    configuration_section: *const u16,
    presentation_owner: u32,
    sr_backend_choices: u32,
    engine_type: u32,
    application_id: u32,
    engine_version_utf8: *const c_char,
    project_id_utf8: *const c_char,
    depth_inverted: u32,
    units_to_meters: f32,
    ui_mode: u32,
    accept_window: WindowFn,
    user: *mut c_void,
}

const _: () = {
    assert!(size_of::<Setup>() == 96 && std::mem::offset_of!(Setup, presentation_owner) == 32);
    assert!(
        std::mem::offset_of!(Setup, accept_window) == 80 && std::mem::offset_of!(Setup, user) == 88
    );
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Adapter {
    vendor: u32,
    device: u32,
    software: u32,
}

// Field order matches `Platform` in render_host_platform.cpp. Returned COM interfaces carry
// one reference for the caller; arguments are borrowed for the call.
#[repr(C)]
pub(crate) struct Platform {
    struct_size: u32,
    tick: unsafe extern "C" fn() -> u64,
    foreground: unsafe extern "C" fn() -> u32,
    hide_cursor: unsafe extern "C" fn() -> i32,
    list_device: unsafe extern "C" fn(Handle) -> Handle,
    queue_device: unsafe extern "C" fn(Handle) -> Handle,
    queue_direct: unsafe extern "C" fn(Handle) -> i32,
    create_fence: unsafe extern "C" fn(Handle) -> Handle,
    fence_completed: unsafe extern "C" fn(Handle) -> u64,
    // 1 completed, 0 no wait event, -1 failed or timed out.
    fence_wait: unsafe extern "C" fn(Handle, u64, u32) -> i32,
    queue_signal: unsafe extern "C" fn(Handle, Handle, u64) -> i32,
    adapter: unsafe extern "C" fn(Handle, *mut Adapter) -> i32,
    native_object: unsafe extern "C" fn(Handle) -> Handle,
    depth_domains: unsafe extern "C" fn(*const Pass) -> u32,
    dlss_preset_valid: unsafe extern "C" fn(u32) -> u32,
    fg_default_backend: unsafe extern "C" fn(u32, u32) -> u32,
    sr12_create: unsafe extern "C" fn(*const SrSetup, *mut Handle) -> i32,
    sr12_destroy: unsafe extern "C" fn(Handle),
    sr12_evaluate: unsafe extern "C" fn(Handle, Handle, *const Pass, u32) -> i32,
    sr12_set_sharpening: unsafe extern "C" fn(Handle, *const Sharpening) -> i32,
    sr12_present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    sr12_capture: unsafe extern "C" fn(Handle, *const c_char, u64, u32) -> i32,
    sr12_capture_flush: unsafe extern "C" fn(Handle, u32) -> i32,
    sr12_capture_scene: unsafe extern "C" fn(Handle, Handle, *const Pass, u32) -> i32,
    registration_create: unsafe extern "C" fn(Handle, *const DlssSetup, *mut Handle) -> i32,
    // Borrows the engine's loaded sl.interposer.dll.
    registration_borrow: unsafe extern "C" fn(Handle, *const DlssSetup, *mut Handle) -> i32,
    registration_present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    fg_install: unsafe extern "C" fn(*const FgSetup) -> i32,
    fg_options: unsafe extern "C" fn(*const FgOptions),
    fg_latency: unsafe extern "C" fn(*const LatencyEvent) -> i32,
    fg_cpu: unsafe extern "C" fn(*const CpuEvent),
    fg_submitted: unsafe extern "C" fn(u64, u64),
    fg_hudless: unsafe extern "C" fn(Handle, *const Pass),
    fg_window: unsafe extern "C" fn(*const Pass),
    fg_status_try: unsafe extern "C" fn(*mut FgStatus) -> i32,
    fg_capture_outputs: unsafe extern "C" fn(*const c_char) -> i32,
    present_host: unsafe extern "C" fn() -> Handle,
    present_has_owner: unsafe extern "C" fn() -> i32,
    present_request: unsafe extern "C" fn(u32) -> i32,
    present_backend_choices: unsafe extern "C" fn() -> u32,
    present_backend: unsafe extern "C" fn() -> u32,
    overlay_visible: unsafe extern "C" fn() -> u32,
    overlay_toggle: unsafe extern "C" fn(),
    overlay_present: unsafe extern "C" fn(
        Handle,
        Handle,
        Handle,
        *const Stats,
        *mut Intent,
        LogFn,
        Handle,
    ) -> i32,
    overlay_fill_fg: unsafe extern "C" fn(*mut Stats, *const FgStatus),
    observer_create: unsafe extern "C" fn(*const ObserverSetup, *mut Handle) -> i32,
    observer_device: unsafe extern "C" fn(Handle, Handle) -> i32,
    observer_factory: unsafe extern "C" fn(Handle, Handle) -> i32,
    diagnostics_enable: unsafe extern "C" fn(LogFn, Handle) -> i32,
}

#[derive(Clone, Copy)]
struct Slot {
    commands: Handle,
    serial: u64,
    fence_value: u64,
    pending: bool,
    capture_retired: bool,
    native_frame: u64,
}

const EMPTY_SLOT: Slot = Slot {
    commands: null_mut(),
    serial: 0,
    fence_value: 0,
    pending: false,
    capture_retired: false,
    native_frame: 0,
};

#[derive(Clone, Copy, Default)]
struct Scene {
    frame: u64,
    source: u64,
    recording: u64,
    surface: usize,
    submitted: bool,
    captured: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct ContextKey {
    backend: u32,
    quality: u32,
    preset: u32,
    exposure: bool,
    render: (u32, u32),
    output: (u32, u32),
}

struct State {
    device: Option<ComRef>,
    fence: Option<ComRef>,
    queue: Option<ComRef>,
    overlay_queue: Option<ComRef>,
    context: Handle,
    // The game retains native NGX handles for pause and fallback. Global SDK shutdown during a
    // backend change invalidates those handles, so the registration belongs to the session.
    dlss_registration: Handle,
    poll_tick: u64,
    serial: u64,
    fence_value: u64,
    // requested_backend is the saved setting, possibly Auto; backend is what it resolves to.
    backend: u32,
    requested_backend: u32,
    preset: u32,
    sharpening: Sharpening,
    enabled: bool,
    reset: bool,
    key: ContextKey,
    settings: Settings,
    fg_backend_requested: u32,
    fg_switch_result: i32,
    slots: [Slot; SLOTS],
    scenes: [Scene; SCENES],
    scene_reports: u32,
    renderer_reason: u32,
    status_overlay: bool,
    feedback_tick: u64,
    native_frame: u64,
    application_presents: u64,
    rates: Rates,
    // Recording serials snapshotted before Execute, so a recycled list cannot retire a newer slot.
    submitting: HashMap<u64, Vec<(usize, u64)>>,
    last_result: i32,
    last_dispatch: i32,
}

#[derive(Default)]
struct Probe {
    enabled: bool,
    phase: u32,
    start: u64,
}

pub(crate) struct Host {
    p: &'static Platform,
    state: Mutex<State>,
    // Touched only from the overlay on the Present path.
    probe: Mutex<Probe>,
    ini: Vec<u16>,
    section: Vec<u16>,
    session: u64,
    sr_choices: u32,
    engine_type: u32,
    application_id: u32,
    engine_version: CString,
    project_id: CString,
    window_filter: WindowFn,
    window_user: *mut c_void,
    streamline: CString,
    interposer: CString,
    fsr: CString,
    xess: CString,
    capture_directory: CString,
    logger: rsf_game_log_fn,
    logger_user: *mut c_void,
    engine_streamline: bool,
    fg_installed: AtomicBool,
    observer: AtomicPtr<c_void>,
    // Auto resolves once against the adapter that owns the game's device.
    automatic_backend: AtomicU32,
    poisoned: AtomicBool,
    latency_reports: AtomicU32,
    observed_commands: [AtomicPtr<c_void>; SLOTS],
    graphics_ready: AtomicBool,
    // Panel layout word, saved under its own key next to the render settings.
    ui_preferences: AtomicU32,
}

// The process-lifetime host behind the frontend entries. It is never freed.
static FRONTEND: AtomicPtr<Host> = AtomicPtr::new(null_mut());
static CREATION: Mutex<()> = Mutex::new(());

fn retain(object: Handle) -> Option<ComRef> {
    (!object.is_null()).then(|| unsafe { ComRef::retain(object) })
}

fn has(mask: u32, bit: u32) -> bool {
    1u32.checked_shl(bit).is_some_and(|bit| mask & bit != 0)
}

// Terminated UTF-16 copy of a terminated wide string.
unsafe fn wide_copy(text: *const u16) -> Vec<u16> {
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    unsafe { std::slice::from_raw_parts(text, length + 1) }.to_vec()
}

fn utf8(text: &[u16]) -> String {
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    String::from_utf16_lossy(&text[..end])
}

fn c_string(text: String) -> CString {
    CString::new(text).unwrap_or_default()
}

// Present-ratio quality from the actual inputs. The game applies resolution changes
// asynchronously, so the backend stays configured for the inputs it actually receives.
fn infer_quality(render_width: u32, output_width: u32) -> u32 {
    let ratio = output_width as f32 / render_width as f32;
    let mut best = 0;
    for (index, candidate) in QUALITY_RATIOS.iter().enumerate().skip(1) {
        if (ratio - candidate).abs() < (ratio - QUALITY_RATIOS[best]).abs() {
            best = index;
        }
    }
    best as u32
}

fn host_from<'a>(user: *mut c_void) -> &'a Host {
    unsafe { &*user.cast::<Host>() }
}

// The published host when `user` is that host.
fn published<'a>(user: *mut c_void) -> Option<&'a Host> {
    let host = FRONTEND.load(Ordering::Acquire);
    (!host.is_null() && host.cast::<c_void>() == user).then(|| unsafe { &*host })
}

fn frontend<'a>() -> Option<&'a Host> {
    let host = FRONTEND.load(Ordering::Acquire);
    (!host.is_null()).then(|| unsafe { &*host })
}

unsafe extern "C" fn backend_log(user: *mut c_void, message: *const c_char) {
    let host = host_from(user);
    if let Some(logger) = host.logger {
        unsafe { logger(host.logger_user, message) };
    }
}

unsafe extern "C" fn accept_window(user: *mut c_void, window: *mut c_void) -> c_int {
    let host = host_from(user);
    c_int::from(
        host.window_filter
            .is_some_and(|filter| unsafe { filter(host.window_user, window) } != 0),
    )
}

impl State {
    fn new() -> Self {
        Self {
            device: None,
            fence: None,
            queue: None,
            overlay_queue: None,
            context: null_mut(),
            dlss_registration: null_mut(),
            poll_tick: 0,
            serial: 0,
            fence_value: 0,
            backend: 0,
            requested_backend: SR_AUTO,
            preset: 0,
            sharpening: unsafe { std::mem::zeroed() },
            enabled: true,
            reset: true,
            key: ContextKey::default(),
            settings: DEFAULTS,
            fg_backend_requested: u32::MAX,
            fg_switch_result: i32::MAX,
            slots: [EMPTY_SLOT; SLOTS],
            scenes: [Scene::default(); SCENES],
            scene_reports: 0,
            renderer_reason: 0,
            status_overlay: true,
            feedback_tick: 0,
            native_frame: 0,
            application_presents: 0,
            rates: Rates::default(),
            submitting: HashMap::new(),
            last_result: i32::MAX,
            last_dispatch: i32::MAX,
        }
    }
}

impl Host {
    fn user(&self) -> Handle {
        ptr::from_ref(self).cast_mut().cast()
    }

    fn log(&self, message: &str) {
        if let (Some(logger), Ok(text)) = (self.logger, CString::new(message)) {
            unsafe { logger(self.logger_user, text.as_ptr()) };
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn try_lock(&self) -> Option<MutexGuard<'_, State>> {
        match self.state.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::Poisoned(state)) => Some(state.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn fg_installed(&self) -> bool {
        self.fg_installed.load(Ordering::Acquire)
    }

    // One section read: the public settings and the stored FG value FG12 takes as its mode.
    fn read_settings_and_fg_mode(&self) -> (Settings, u32) {
        let defaults = sharpening_policy::defaults();
        let (mut value, fg_mode) = unsafe {
            read_settings_and_fg_mode(
                self.ini.as_ptr(),
                self.section.as_ptr(),
                &DEFAULTS,
                &defaults,
            )
        }
        .unwrap_or((DEFAULTS, DEFAULTS.fg_enabled));
        let options = sharpening_policy::sanitize(sharpening_of(&value, &defaults));
        set_sharpening(&mut value, &options);
        (value, fg_mode)
    }

    fn adapter(&self, s: &State) -> Option<Adapter> {
        let device = s.device.as_ref()?;
        let mut adapter = Adapter::default();
        (unsafe { (self.p.adapter)(device.as_ptr(), &mut adapter) } != 0).then_some(adapter)
    }

    fn resolve_backend(&self, s: &State) -> u32 {
        if s.requested_backend != SR_AUTO {
            return s.requested_backend;
        }
        let selected = self.automatic_backend.load(Ordering::Relaxed);
        if selected != 0 {
            return selected;
        }
        let Some(adapter) = self.adapter(s) else {
            return SR_NONE;
        };
        let selected =
            crate::rsf_gpu_select_sr_backend(adapter.vendor, adapter.software, self.sr_choices);
        self.automatic_backend.store(selected, Ordering::Relaxed);
        self.log(&format!(
            "Auto SR selected backend {selected} from owning adapter vendor={:#x} device={:#x}",
            adapter.vendor, adapter.device
        ));
        selected
    }

    // Applies the saved settings at most once per second, or at once after a commit.
    fn poll(&self, s: &mut State) {
        let p = self.p;
        let now = unsafe { (p.tick)() };
        if now < s.poll_tick {
            return;
        }
        s.poll_tick = now + 1000;
        let (values, fg_mode) = self.read_settings_and_fg_mode();
        s.settings = values;
        s.sharpening = sharpening_of(&values, &sharpening_policy::defaults());
        if !s.context.is_null() {
            let _ = unsafe { (p.sr12_set_sharpening)(s.context, &s.sharpening) };
        }
        s.enabled = values.sr_enabled != 0;
        s.requested_backend = values.sr_backend;
        s.backend = self.resolve_backend(s);
        s.preset = values.dlss_preset;
        s.status_overlay = values.status_overlay != 0;
        let layout =
            unsafe { rsf_settings_store_read_layout(self.ini.as_ptr(), self.section.as_ptr()) };
        self.ui_preferences.store(layout, Ordering::Relaxed);
        if !self.fg_installed() {
            return;
        }
        let options = FgOptions {
            struct_size: size_of::<FgOptions>() as u32,
            // The stored value, not fg_enabled: 2 and 3 are RSF_FG_AUTO and RSF_FG_DYNAMIC.
            mode: fg_mode,
            generated_frames: values.generated_frames,
            reflex_mode: values.latency,
            frame_limit_us: values.frame_limit_us,
            debug: 0,
        };
        unsafe { (p.fg_options)(&options) };
        let mut desired = values.fg_backend;
        if desired == RSF_FG_BACKEND_AUTO && s.device.is_some() {
            let vendor = self.adapter(s).map_or(0, |adapter| adapter.vendor);
            let preferred = unsafe { (p.fg_default_backend)(vendor, 0) };
            let choices = unsafe { (p.present_backend_choices)() };
            // Honor the bridge's Off choice when the adapter has no preference.
            desired = if preferred == 0 && has(choices, 0) {
                0
            } else {
                fg_choice::rsf_fg_select_auto_backend(preferred, choices)
            };
        }
        if desired != RSF_FG_BACKEND_AUTO
            && desired != s.fg_backend_requested
            && unsafe { (p.present_has_owner)() } != 0
        {
            let result = unsafe { (p.present_request)(desired) };
            if result == BACKEND_OK {
                s.fg_backend_requested = desired;
            }
            if result != s.fg_switch_result {
                self.log(&format!(
                    "Native renderer FG backend request={desired} result={result}."
                ));
                s.fg_switch_result = result;
            }
        }
    }

    fn completed(&self, fence: &ComRef) -> Option<u64> {
        let completed = unsafe { (self.p.fence_completed)(fence.as_ptr()) };
        if completed == u64::MAX {
            self.poisoned.store(true, Ordering::Relaxed);
            None
        } else {
            Some(completed)
        }
    }

    fn wait(&self, s: &State, value: u64) -> bool {
        if value == 0 {
            return true;
        }
        let Some(fence) = s.fence.as_ref() else {
            return false;
        };
        let Some(completed) = self.completed(fence) else {
            return false;
        };
        if completed >= value {
            return true;
        }
        match unsafe { (self.p.fence_wait)(fence.as_ptr(), value, FENCE_TIMEOUT_MS) } {
            0 => false,
            result if result > 0 => self.completed(fence).is_some_and(|done| done >= value),
            _ => {
                self.poisoned.store(true, Ordering::Relaxed);
                false
            }
        }
    }

    // Never waits for work the engine has not submitted. Its producer might be this worker.
    fn drain(&self, s: &State) -> bool {
        !s.slots.iter().any(|slot| slot.pending) && self.wait(s, s.fence_value)
    }

    fn flush_captures(&self, s: &mut State) {
        if s.context.is_null() || self.capture_directory.is_empty() {
            return;
        }
        let Some(fence) = s.fence.as_ref() else {
            return;
        };
        let Some(completed) = self.completed(fence) else {
            return;
        };
        for (index, slot) in s.slots.iter_mut().enumerate() {
            if !slot.pending
                && !slot.capture_retired
                && slot.fence_value != 0
                && completed >= slot.fence_value
            {
                slot.capture_retired =
                    unsafe { (self.p.sr12_capture_flush)(s.context, index as u32) == BACKEND_OK };
            }
        }
    }

    // Replaces the SR context for a changed backend, quality, preset, exposure or extent.
    fn configure(&self, s: &mut State, pass: &Pass, key: ContextKey) -> i32 {
        let quality = key.quality;
        let p = self.p;
        let camera = &pass.camera;
        let device = s.device.as_ref().map_or(null_mut(), ComRef::as_ptr);
        let mut setup = SrSetup {
            struct_size: size_of::<SrSetup>() as u32,
            abi_version: 2,
            device,
            backend: s.backend,
            quality,
            output_width: camera.output_width,
            output_height: camera.output_height,
            render_width: camera.render_width,
            render_height: camera.render_height,
            engine_input_extents: 1,
            inverted_depth: camera.depth_inverted,
            external_exposure: u32::from(!pass.exposure.is_null()),
            fsr2_directory_utf8: self.fsr.as_ptr(),
            fsr3_directory_utf8: self.fsr.as_ptr(),
            fsr4_directory_utf8: self.fsr.as_ptr(),
            xess_directory_utf8: self.xess.as_ptr(),
            log: Some(backend_log),
            user: self.user(),
            ..SrSetup::default()
        };
        setup.dlss = DlssSetup {
            struct_size: size_of::<DlssSetup>() as u32,
            abi_version: RSF_DLSS_ABI_VERSION,
            interposer_path_utf8: self.interposer.as_ptr(),
            plugin_directory_utf8: self.streamline.as_ptr(),
            engine: self.engine_type,
            engine_version_utf8: self.engine_version.as_ptr(),
            project_id_utf8: if self.project_id.is_empty() {
                null()
            } else {
                self.project_id.as_ptr()
            },
            application_id: self.application_id,
            require_signature: 1,
            log: Some(backend_log),
            log_user: self.user(),
            preset: s.preset,
            ..DlssSetup::default()
        };
        setup.streamline_host = unsafe { (p.present_host)() };
        // The scene is linear floating-point HDR even when the swapchain is SDR.
        if s.backend == SR_DLSS && setup.streamline_host.is_null() {
            if s.dlss_registration.is_null() {
                let mut registration = null_mut();
                let registered = unsafe {
                    if self.engine_streamline {
                        (p.registration_borrow)(device, &setup.dlss, &mut registration)
                    } else {
                        (p.registration_create)(device, &setup.dlss, &mut registration)
                    }
                };
                if registered != BACKEND_OK {
                    return registered;
                }
                s.dlss_registration = registration;
                self.log(
                    "Native renderer NGX registration retained for native game handles across backend changes.",
                );
            }
            setup.dlss_registration = s.dlss_registration;
        }
        // Retire this SR feature's resources after GPU completion. Its registration remains
        // alive for native evaluation and for a later SR context on the same device.
        if !s.context.is_null() {
            unsafe { (p.sr12_destroy)(s.context) };
            s.context = null_mut();
        }
        let mut replacement = null_mut();
        let result = unsafe { (p.sr12_create)(&setup, &mut replacement) };
        if result != BACKEND_OK {
            return result;
        }
        s.context = replacement;
        let _ = unsafe { (p.sr12_set_sharpening)(s.context, &s.sharpening) };
        if !self.capture_directory.is_empty()
            && unsafe {
                (p.sr12_capture)(
                    s.context,
                    self.capture_directory.as_ptr(),
                    pass.native_frame,
                    4,
                )
            } != BACKEND_OK
        {
            self.log("Native renderer bounded SR capture configuration refused.");
        }
        s.key = key;
        s.reset = true;
        self.log(&format!(
            "Native renderer SR context backend={} quality={} input={}x{} output={}x{}.",
            key.backend, key.quality, key.render.0, key.render.1, key.output.0, key.output.1
        ));
        BACKEND_OK
    }

    fn process(&self, commands: Handle, pass: *const Pass) -> i32 {
        let p = self.p;
        if pass.is_null() || unsafe { pass.cast::<u32>().read() } < size_of::<Pass>() as u32 {
            return INVALID;
        }
        let pass = unsafe { &*pass };
        if commands.is_null() || pass.camera_valid == 0 || pass.session_id != self.session {
            return INVALID;
        }
        if unsafe { (p.depth_domains)(pass) } != 0 {
            return INVALID;
        }
        let mut guard = self.lock();
        let s = &mut *guard;
        self.flush_captures(s);
        self.poll(s);
        if self.poisoned.load(Ordering::Relaxed)
            || !s.enabled
            || unsafe { (p.dlss_preset_valid)(s.preset) } == 0
        {
            s.reset = true;
            return NOT_READY;
        }
        let Some(owner) = (unsafe { ComRef::from_owned((p.list_device)(commands)) }) else {
            return NOT_READY;
        };
        if s.device.is_none() {
            let Some(fence) = (unsafe { ComRef::from_owned((p.create_fence)(owner.as_ptr())) })
            else {
                return NOT_READY;
            };
            s.device = Some(owner.clone());
            s.fence = Some(fence);
        }
        if s.device
            .as_ref()
            .is_some_and(|device| device.as_ptr() != owner.as_ptr())
        {
            return NATIVE_REFUSED;
        }
        drop(owner);
        // Auto needs the owning adapter, which is known only once the game records a pass.
        s.backend = self.resolve_backend(s);
        if !(SR_DLSS..=SR_XESS).contains(&s.backend) || !has(self.sr_choices, s.backend) {
            s.reset = true;
            return NOT_READY;
        }
        let camera = &pass.camera;
        let key = ContextKey {
            backend: s.backend,
            quality: infer_quality(camera.render_width, camera.output_width),
            preset: s.preset,
            exposure: !pass.exposure.is_null(),
            render: (camera.render_width, camera.render_height),
            output: (camera.output_width, camera.output_height),
        };
        if s.context.is_null() || s.key != key {
            if !self.drain(s) {
                return BUSY;
            }
            let result = self.configure(s, pass, key);
            if result != BACKEND_OK {
                return result;
            }
        }
        let Some(completed) = s.fence.as_ref().and_then(|fence| self.completed(fence)) else {
            s.reset = true;
            return NOT_READY;
        };
        let mut index = s
            .slots
            .iter()
            .position(|slot| !slot.pending && completed >= slot.fence_value);
        if index.is_none() {
            // A submitted slot can be joined. An unsubmitted one must never block its producer.
            index =
                (0..SLOTS).find(|&i| !s.slots[i].pending && self.wait(s, s.slots[i].fence_value));
        }
        let Some(index) = index else {
            s.reset = true;
            return BUSY;
        };
        let mut copied = *pass;
        if s.reset {
            copied.flags |= RSF_GAME_RENDER_RESET;
        }
        // Reserve before evaluation: a failure can still contain normalization or vendor commands.
        s.serial += 1;
        s.slots[index] = Slot {
            commands,
            serial: s.serial,
            fence_value: 0,
            pending: true,
            capture_retired: false,
            native_frame: pass.native_frame,
        };
        self.observed_commands[index].store(commands, Ordering::Release);
        let result = unsafe { (p.sr12_evaluate)(s.context, commands, &copied, index as u32) };
        s.reset = result != BACKEND_OK;
        if result != s.last_result {
            self.log(&format!(
                "Native renderer RSF SR evaluation result={result} frame={}.",
                pass.native_frame
            ));
            s.last_result = result;
        }
        result
    }

    fn submit(&self, native_queue: Handle, id: u64, count: u32, lists: *const Handle, begin: bool) {
        let p = self.p;
        let mut guard = self.lock();
        let s = &mut *guard;
        if native_queue.is_null() {
            return;
        }
        // The plugin observes only the renderer's selected DIRECT queue. Retain it before the
        // first upscale so the overlay also works on the opening menus.
        if s.queue.is_none() && !self.engine_streamline {
            s.queue = retain(native_queue);
        }
        if s.context.is_null() {
            return;
        }
        let Some(fence) = s.fence.as_ref().map(ComRef::as_ptr) else {
            return;
        };
        if begin {
            let lists = if lists.is_null() || count == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(lists, count as usize) }
            };
            let snapshot: Vec<(usize, u64)> = s
                .slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.pending && lists.contains(&slot.commands))
                .map(|(index, slot)| (index, slot.serial))
                .collect();
            if !snapshot.is_empty()
                && let Entry::Vacant(entry) = s.submitting.entry(id)
            {
                entry.insert(snapshot);
            }
            return;
        }
        let Some(entry) = s.submitting.remove(&id) else {
            return;
        };
        if s.queue
            .as_ref()
            .is_some_and(|queue| queue.as_ptr() != native_queue)
        {
            self.poisoned.store(true, Ordering::Relaxed);
            self.log("Native renderer SR refused: its recordings reached multiple DIRECT queues.");
            return;
        }
        if s.queue.is_none() {
            s.queue = retain(native_queue);
        }
        s.fence_value += 1;
        let value = s.fence_value;
        if unsafe { (p.queue_signal)(native_queue, fence, value) } == 0 {
            self.poisoned.store(true, Ordering::Relaxed);
            self.log("Native renderer SR queue fence signal failed; GPU resources remain pinned.");
            return;
        }
        for (index, recording) in entry {
            let slot = &mut s.slots[index];
            if slot.pending && slot.serial == recording {
                slot.pending = false;
                slot.fence_value = value;
                self.observed_commands[index].store(null_mut(), Ordering::Release);
            }
            let frame = slot.native_frame;
            let scene = &mut s.scenes[(frame % SCENES as u64) as usize];
            if scene.frame == frame && scene.recording == recording {
                scene.submitted = true;
                unsafe { (p.fg_submitted)(self.session, scene.source) };
            }
        }
    }

    fn scene(&self, commands: Handle, pass: &Pass) {
        let mut guard = self.lock();
        let s = &mut *guard;
        if s.context.is_null()
            || pass.color_input.is_null()
            || pass.scene_surface.is_null()
            || pass.native_frame == 0
        {
            return;
        }
        let Some(index) = s.slots.iter().position(|slot| {
            slot.pending && slot.commands == commands && slot.native_frame == pass.native_frame
        }) else {
            return;
        };
        let result =
            unsafe { (self.p.sr12_capture_scene)(s.context, commands, pass, index as u32) };
        s.scenes[(pass.native_frame % SCENES as u64) as usize] = Scene {
            frame: pass.native_frame,
            source: pass.source_frame_id,
            recording: s.slots[index].serial,
            surface: pass.scene_surface as usize,
            submitted: false,
            captured: result == BACKEND_OK,
        };
        if pass.source_frame_id != 0 {
            unsafe { (self.p.fg_hudless)(commands, pass) };
        }
    }

    fn window(&self, pass: &Pass) {
        let mut guard = self.lock();
        let s = &mut *guard;
        if pass.native_frame == 0 {
            return;
        }
        let scene = s.scenes[(pass.native_frame % SCENES as u64) as usize];
        let matched = scene.frame == pass.native_frame
            && scene.surface == pass.scene_surface as usize
            && scene.submitted;
        if matched && pass.source_frame_id != 0 && scene.source == pass.source_frame_id {
            unsafe { (self.p.fg_window)(pass) };
        }
        let report = s.scene_reports < 8;
        s.scene_reports += 1;
        if report || scene.captured {
            self.log(&format!(
                "Native renderer scene/Present frame={} input={} submitted={} surface_match={} captured={}.",
                pass.native_frame,
                pass.source_frame_id,
                u32::from(scene.submitted),
                u32::from(matched),
                u32::from(scene.captured)
            ));
        }
    }

    fn present(&self, swapchain: Handle, interval: u32, flags: u32, begin: bool) {
        let test = flags & PRESENT_TEST != 0;
        if begin && !test {
            self.overlay(swapchain);
        }
        let p = self.p;
        let mut guard = self.lock();
        let s = &mut *guard;
        self.flush_captures(s);
        if !begin && !test {
            s.application_presents += 1;
        }
        let begin = u32::from(begin);
        let result = unsafe {
            if !s.dlss_registration.is_null() {
                (p.registration_present)(s.dlss_registration, swapchain, interval, flags, begin)
            } else if !s.context.is_null() {
                (p.sr12_present)(s.context, swapchain, interval, flags, begin)
            } else {
                BACKEND_OK
            }
        };
        if result != BACKEND_OK {
            self.poisoned.store(true, Ordering::Relaxed);
            self.log(
                "Native renderer SR manual Present bookkeeping failed; native evaluation retained.",
            );
        }
    }

    // The frontend status from saved `values`, the FG snapshot and the host state.
    fn status(&self, s: &mut State, values: &Settings, fg: &FgStatus, now: u64) -> Status {
        let p = self.p;
        let backend = unsafe { (p.present_backend)() };
        let valid =
            fg.available != 0 && fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0;
        s.rates.sample(
            s.application_presents,
            backend,
            fg.vendor.total_presented,
            valid,
            now,
        );
        let recent = s.feedback_tick != 0 && now.wrapping_sub(s.feedback_tick) < 1000;
        let mut value = Status {
            struct_size: size_of::<Status>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: u32::from(s.enabled),
            sr_requested_backend: s.backend,
            sr_effective_backend: if s.enabled
                && s.renderer_reason == 0
                && s.last_dispatch == 0
                && now.wrapping_sub(s.feedback_tick) < 1000
            {
                s.key.backend
            } else {
                0
            },
            render_width: s.key.render.0,
            render_height: s.key.render.1,
            output_width: s.key.output.0,
            output_height: s.key.output.1,
            fg_requested_backend: values.fg_backend,
            fg_effective_backend: backend,
            fg_requested: u32::from(values.fg_enabled != 0),
            fg_active: fg.vendor.active,
            requested_generated: values.generated_frames,
            effective_generated: fg.vendor.effective_generated_frames,
            max_generated: fg.vendor.max_generated_frames,
            foreground: unsafe { (p.foreground)() },
            latency_requested: values.latency,
            latency_effective: fg.vendor.effective_reflex,
            renderer_reason: s.renderer_reason,
            fg_reason: fg.reason,
            sr_result: s.last_dispatch,
            fg_result: fg.last_result,
            fg_vendor_result: fg.vendor.vendor_status,
            output_fps_valid: u32::from(s.rates.output_valid && valid),
            status_overlay: u32::from(s.status_overlay),
            sr_backend_choices: self.sr_choices,
            fg_backend_choices: unsafe { (p.present_backend_choices)() },
            base_fps: s.rates.base_fps,
            output_fps: s.rates.output_fps,
            native_frame: s.native_frame,
            source_frame: fg.source_frame_id,
            tagged_frames: fg.tagged_frames,
            application_presents: s.application_presents,
            output_presents: fg.vendor.total_presented,
            ..Status::default()
        };
        if value.fg_requested == 0 {
            value.fg_requested_backend = 0;
            if fg.vendor.configured_mode == RSF_FG_OFF {
                value.fg_effective_backend = 0;
            }
        }
        let reason = if value.fg_requested == 0 {
            "Disabled by setting"
        } else if value.sr_enabled == 0 {
            "RSF SR input path is disabled"
        } else if self.poisoned.load(Ordering::Relaxed) {
            "GPU submission or presentation failed"
        } else if recent && s.renderer_reason != 0 {
            RENDERER_REASONS
                .get(s.renderer_reason as usize)
                .copied()
                .unwrap_or("Renderer refused frame")
        } else if fg.available == 0 {
            "FG presentation unavailable"
        } else if fg.last_result != 0 {
            "FG backend error"
        } else if fg.reason == RSF_NATIVE_FG_REASON_RESET {
            "Resetting temporal history"
        } else if fg.reason == RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED {
            "Scene is not eligible"
        } else if fg.reason == RSF_NATIVE_FG_REASON_HUDLESS_MISSING {
            "HUD-less scene missing"
        } else if fg.reason != 0 {
            "Frame inputs or submission not matched"
        } else if fg.vendor.active != 0 {
            "Active"
        } else if value.foreground == 0 {
            "Game is in background"
        } else {
            "SDK reports inactive"
        };
        for (target, &byte) in value.reason.iter_mut().zip(reason.as_bytes()) {
            *target = byte as c_char;
        }
        value
    }

    fn empty_fg() -> FgStatus {
        FgStatus {
            struct_size: size_of::<FgStatus>() as u32,
            ..unsafe { std::mem::zeroed() }
        }
    }

    fn status_get(&self) -> Result<Status, i32> {
        let mut fg = Self::empty_fg();
        if unsafe { (self.p.fg_status_try)(&mut fg) } == 0 && self.fg_installed() {
            return Err(BUSY);
        }
        let mut state = self.try_lock().ok_or(BUSY)?;
        let values = state.settings;
        let now = unsafe { (self.p.tick)() };
        Ok(self.status(&mut state, &values, &fg, now))
    }

    fn settings_get(&self) -> Result<Settings, i32> {
        Ok(self.try_lock().ok_or(BUSY)?.settings)
    }

    // Validated against the whole request, then saved in one section write and applied.
    fn settings_apply(&self, request: &Settings) -> i32 {
        let p = self.p;
        let sharpening = sharpening_of(request, &sharpening_policy::defaults());
        if !sharpening_policy::valid(&sharpening)
            || request.sr_enabled > 1
            || request.fg_enabled > 1
            || request.status_overlay > 1
            || request.latency > 2
            || request.sr_quality > 4
            || !(1..=15).contains(&request.generated_frames)
            || unsafe { (p.dlss_preset_valid)(request.dlss_preset) } == 0
            || (request.sr_backend != SR_AUTO
                && (request.sr_backend > 31 || !has(self.sr_choices, request.sr_backend)))
            || !fg_choice::allowed(request.fg_backend, RSF_FG_BACKEND_ALL)
        {
            return INVALID;
        }
        let Some(mut state) = self.try_lock() else {
            return BUSY;
        };
        let written =
            unsafe { rsf_settings_store_write(self.ini.as_ptr(), self.section.as_ptr(), request) };
        if written != OK {
            return written;
        }
        state.poll_tick = 0;
        self.poll(&mut state);
        OK
    }

    fn overlay_probe(&self, application_presents: u64) {
        let p = self.p;
        let mut probe = self.probe.lock().unwrap_or_else(PoisonError::into_inner);
        if !probe.enabled {
            return;
        }
        if probe.phase == 0 && application_presents >= 60 {
            unsafe { (p.overlay_toggle)() };
            if unsafe { (p.overlay_visible)() } != 0 {
                probe.phase = 1;
                probe.start = application_presents;
                self.log("Native renderer overlay probe: opening shared panel.");
                let mut attempts = 0u32;
                let count = loop {
                    let count = unsafe { (p.hide_cursor)() };
                    attempts += 1;
                    if count < 0 || attempts >= 512 {
                        break count;
                    }
                };
                if count >= 0 {
                    probe.phase = 3;
                    self.log(
                        "Native renderer overlay probe FAILED: cursor hide loop did not terminate.",
                    );
                } else {
                    self.log("Native renderer overlay probe: game-style cursor hide loop returned negative.");
                }
            }
        } else if probe.phase == 1 && application_presents.wrapping_sub(probe.start) >= 120 {
            probe.phase = 2;
            if unsafe { (p.overlay_visible)() } != 0 {
                unsafe { (p.overlay_toggle)() };
            }
            self.log("Native renderer overlay probe: full panel survived 120 application Presents; closed.");
        }
        if probe.phase == 1 {
            self.log(&format!(
                "Native renderer overlay probe present={application_presents}"
            ));
        }
    }

    fn overlay(&self, chain: Handle) {
        thread_local! {
            static STATUS: Cell<Status> = Cell::new(Status {
                struct_size: size_of::<Status>() as u32,
                abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
                ..Status::default()
            });
            static FG: Cell<FgStatus> = Cell::new(Host::empty_fg());
            // Upscale workers own the renderer lock for most of a busy frame. A busy settings
            // observer must not remove the HUD or panel from that frame.
            static PREFS: Cell<Option<Settings>> = const { Cell::new(None) };
        }
        let p = self.p;
        let (queue, quality) = {
            let s = self.lock();
            (
                s.overlay_queue.clone().or_else(|| s.queue.clone()),
                s.key.quality,
            )
        };
        let Some(queue) = queue else {
            return;
        };
        let Some(device) = (unsafe { ComRef::from_owned((p.queue_device)(queue.as_ptr())) }) else {
            return;
        };
        let mut fresh = Self::empty_fg();
        let fg_fresh = unsafe { (p.fg_status_try)(&mut fresh) } != 0;
        // One published snapshot serves both status and panel settings.
        if let Some(mut state) = self.try_lock() {
            let values = state.settings;
            if fg_fresh || !self.fg_installed() {
                let now = unsafe { (p.tick)() };
                STATUS.set(self.status(&mut state, &values, &fresh, now));
            }
            PREFS.set(Some(values));
        }
        if fg_fresh {
            FG.set(fresh);
        }
        let status = STATUS.get();
        let fg = FG.get();
        self.overlay_probe(status.application_presents);
        let Some(mut prefs) = PREFS.get() else {
            return;
        };
        let mut stats = Stats {
            struct_size: size_of::<Stats>() as u32,
            backend_loaded: 1,
            backend_supported: u32::from(status.sr_effective_backend != 0),
            ..Stats::default()
        };
        // Auto is shown as the provider it resolved to. Picking a provider in the panel saves it
        // explicitly.
        stats.backend = if prefs.sr_backend == SR_AUTO {
            self.automatic_backend.load(Ordering::Relaxed)
        } else {
            prefs.sr_backend
        };
        stats.backend_name = sr_name(stats.backend).as_ptr();
        stats.requested_backend = stats.backend;
        stats.enabled = prefs.sr_enabled;
        stats.quality = quality;
        stats.sr_backend_choices = self.sr_choices;
        stats.sharpening_available = 1;
        fill_sharpening_stats(
            &mut stats,
            &sharpening_of(&prefs, &sharpening_policy::defaults()),
        );
        stats.have_depth = u32::from(status.sr_effective_backend != 0);
        stats.dlss_preset = prefs.dlss_preset;
        stats.dlss_preset_available = 1;
        stats.refusal_reason = if status.fg_requested != 0 && status.fg_active == 0 {
            status.reason.as_ptr()
        } else {
            null()
        };
        stats.render_width = status.render_width;
        stats.render_height = status.render_height;
        stats.output_width = status.output_width;
        stats.output_height = status.output_height;
        stats.application_presented_frames = status.application_presents;
        stats.show_performance_hud = prefs.status_overlay;
        stats.ui_preferences = self.ui_preferences.load(Ordering::Relaxed);
        let (mut now, mut frequency) = (0i64, 0i64);
        unsafe {
            QueryPerformanceCounter(&mut now);
            QueryPerformanceFrequency(&mut frequency);
        }
        stats.sample_qpc = now as u64;
        stats.qpc_frequency = frequency as u64;
        unsafe { (p.overlay_fill_fg)(&mut stats, &fg) };
        // Keep the saved provider for re-enabling FG, but Off is the selected UI choice while
        // disabled. A retained SDK context is not an active provider.
        stats.fg_requested_backend = if prefs.fg_enabled == 0 {
            0
        } else if prefs.fg_backend == RSF_FG_BACKEND_AUTO {
            stats.fg_backend
        } else {
            prefs.fg_backend
        };
        if prefs.fg_enabled == 0 && fg.vendor.configured_mode == RSF_FG_OFF {
            stats.fg_backend = 0;
        }
        stats.fg_backend_choices = unsafe { (p.present_backend_choices)() }
            | RSF_OVERLAY_FG_RUNTIME_SWITCH
            | 1
            | (1 << RSF_FG_BACKEND_AUTO);
        stats.fg_requested_mode = if prefs.fg_enabled != 0 {
            RSF_FG_FIXED
        } else {
            RSF_FG_OFF
        };
        stats.fg_requested_generated = prefs.generated_frames;
        stats.reflex_requested_mode = prefs.latency;
        stats.frame_limit_us = prefs.frame_limit_us;
        let mut intent = Intent {
            struct_size: size_of::<Intent>() as u32,
            ..Intent::default()
        };
        let shown = unsafe {
            (p.overlay_present)(
                device.as_ptr(),
                queue.as_ptr(),
                chain,
                &stats,
                &mut intent,
                Some(backend_log),
                self.user(),
            )
        };
        if shown == 0 {
            return;
        }
        if intent.ui_preferences_changed != 0 && intent.ui_preferences != stats.ui_preferences {
            self.ui_preferences
                .store(intent.ui_preferences, Ordering::Relaxed);
            let saved = unsafe {
                rsf_settings_store_write_layout(
                    self.ini.as_ptr(),
                    self.section.as_ptr(),
                    intent.ui_preferences,
                )
            };
            if saved != OK {
                self.log("RSF overlay: panel layout applied for this session; the save failed.");
            }
        }
        if !settings_changed(&intent) {
            return;
        }
        apply_settings_intent(&mut prefs, &intent);
        if self.settings_apply(&prefs) == OK {
            PREFS.set(Some(prefs));
        } else {
            self.log("RSF overlay settings request refused; saved settings retained.");
        }
    }
}

// Service callbacks use the published host as their user pointer.
unsafe extern "C" fn process(user: *mut c_void, commands: Handle, pass: *const Pass) -> i32 {
    let host = host_from(user);
    let result = host.process(commands, pass);
    let mut s = host.lock();
    if result != s.last_dispatch {
        host.log(&format!(
            "Native renderer SR host dispatch result={result}; {}.",
            if result == 0 {
                "RSF output accepted"
            } else {
                "native evaluation retained"
            }
        ));
        s.last_dispatch = result;
    }
    result
}

unsafe extern "C" fn submit(
    user: *mut c_void,
    queue: Handle,
    id: u64,
    count: u32,
    lists: *const Handle,
    begin: u32,
) {
    host_from(user).submit(queue, id, count, lists, begin != 0);
}

unsafe extern "C" fn observer_submit(
    user: *mut c_void,
    queue: Handle,
    id: u64,
    count: u32,
    lists: *const Handle,
    begin: u32,
) {
    let host = host_from(user);
    let lists_slice = if lists.is_null() || count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(lists, count as usize) }
    };
    // Vendor initialization can submit its private lists while the renderer lock is held. Only
    // adapter recordings need retirement through this host.
    let tracked = host.observed_commands.iter().any(|entry| {
        let pointer = entry.load(Ordering::Acquire);
        !pointer.is_null() && lists_slice.contains(&pointer)
    });
    if tracked {
        host.submit(queue, id, count, lists, begin != 0);
    }
}

unsafe extern "C" fn observer_chain(user: *mut c_void, queue: Handle, _: Handle, _: Handle) {
    let host = host_from(user);
    let mut s = host.lock();
    s.overlay_queue = retain(queue);
    host.graphics_ready.store(true, Ordering::Release);
    host.log("Native graphics observer selected the adapter's window and presentation queue.");
}

unsafe extern "C" fn present(
    user: *mut c_void,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) {
    host_from(user).present(swapchain, interval, flags, begin != 0);
}

unsafe extern "C" fn render_pass(
    user: *mut c_void,
    commands: Handle,
    pass: *const Pass,
    begin: u32,
) {
    if pass.is_null() || unsafe { pass.cast::<u32>().read() } < size_of::<Pass>() as u32 {
        return;
    }
    let host = host_from(user);
    let pass = unsafe { &*pass };
    if pass.role == RSF_GAME_RENDER_FINAL_SCENE && begin == 0 {
        host.scene(commands, pass);
    }
    if pass.role == RSF_GAME_RENDER_WINDOW && begin != 0 {
        host.window(pass);
    }
}

unsafe extern "C" fn cpu_event(user: *mut c_void, event: *const CpuEvent) {
    unsafe { (host_from(user).p.fg_cpu)(event) };
}

unsafe extern "C" fn process_latency(user: *mut c_void, event: *const LatencyEvent) -> i32 {
    let host = host_from(user);
    if event.is_null()
        || unsafe { event.cast::<u32>().read() } < size_of::<LatencyEvent>() as u32
        || unsafe { (*event).session_id } != host.session
    {
        return INVALID;
    }
    {
        let mut s = host.lock();
        host.poll(&mut s);
    }
    let result = if host.fg_installed() {
        unsafe { (host.p.fg_latency)(event) }
    } else {
        BACKEND_NOT_READY
    };
    if result == BACKEND_OK && host.latency_reports.fetch_add(1, Ordering::Relaxed) < 12 {
        let event = unsafe { &*event };
        host.log(&format!(
            "Native renderer latency boundary={} source={} thread={} owned=1.",
            event.marker,
            event.source_frame_id,
            unsafe { GetCurrentThreadId() }
        ));
    }
    result
}

unsafe extern "C" fn render_feedback(user: *mut c_void, event: *const Feedback) {
    let host = host_from(user);
    if event.is_null()
        || unsafe { event.cast::<u32>().read() } < size_of::<Feedback>() as u32
        || unsafe { (*event).session_id } != host.session
    {
        return;
    }
    let event = unsafe { &*event };
    let mut s = host.lock();
    s.renderer_reason = event.reason;
    s.feedback_tick = unsafe { (host.p.tick)() };
    if event.native_frame != 0 {
        s.native_frame = event.native_frame;
    }
}

// Admission and the immutable host description. Nothing is installed yet.
unsafe fn describe(setup: &Setup, services: &Services, p: &'static Platform) -> Host {
    let ini = unsafe { wide_copy(setup.configuration_path) };
    let section = unsafe { wide_copy(setup.configuration_section) };
    let probe =
        unsafe { GetPrivateProfileIntW(section.as_ptr(), w!("OverlayProbe"), 0, ini.as_ptr()) }
            != 0;
    let mut capture = vec![0u16; CAPTURE_PATH];
    let length = unsafe {
        GetPrivateProfileStringW(
            section.as_ptr(),
            w!("SRCaptureDirectory"),
            w!(""),
            capture.as_mut_ptr(),
            CAPTURE_PATH as u32,
            ini.as_ptr(),
        )
    } as usize;
    capture.truncate(length);
    let root = utf8(&unsafe { wide_copy(setup.runtime_directory) });
    let streamline = format!("{root}/streamline");
    let interposer = format!("{streamline}/sl.interposer.dll");
    let text = |value: *const c_char| {
        if value.is_null() {
            CString::default()
        } else {
            unsafe { std::ffi::CStr::from_ptr(value) }.to_owned()
        }
    };
    let host = Host {
        p,
        state: Mutex::new(State::new()),
        probe: Mutex::new(Probe {
            enabled: probe,
            ..Probe::default()
        }),
        ini,
        section,
        session: services.session_id,
        sr_choices: setup.sr_backend_choices,
        engine_type: setup.engine_type,
        application_id: setup.application_id,
        engine_version: text(setup.engine_version_utf8),
        project_id: text(setup.project_id_utf8),
        window_filter: setup.accept_window,
        window_user: setup.user,
        streamline: c_string(streamline),
        interposer: c_string(interposer),
        fsr: c_string(format!("{root}/fidelityfx")),
        xess: c_string(format!("{root}/xess")),
        capture_directory: c_string(utf8(&capture)),
        logger: services.log,
        logger_user: services.user,
        engine_streamline: setup.presentation_owner == ENGINE_PRESENTATION,
        fg_installed: AtomicBool::new(false),
        observer: AtomicPtr::new(null_mut()),
        automatic_backend: AtomicU32::new(0),
        poisoned: AtomicBool::new(false),
        latency_reports: AtomicU32::new(0),
        observed_commands: [const { AtomicPtr::new(null_mut()) }; SLOTS],
        graphics_ready: AtomicBool::new(false),
        ui_preferences: AtomicU32::new(0),
    };
    host.lock().settings = host.read_settings_and_fg_mode().0;
    host
}

// Installs the host's diagnostics, FG presentation or graphics observer. The host address is
// final: every callback receives it as its user.
fn install(host: &Host, setup: &Setup) -> i32 {
    let p = host.p;
    let ini = host.ini.as_ptr();
    let section = host.section.as_ptr();
    if unsafe { GetPrivateProfileIntW(section, w!("GraphicsDiagnostics"), 0, ini) } != 0 {
        let _ = unsafe { (p.diagnostics_enable)(Some(backend_log), host.user()) };
    }
    // A diagnostic request belongs to this one process, never the next game launch.
    let probe = host
        .probe
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .enabled;
    if probe
        && unsafe { WritePrivateProfileStringW(section, w!("OverlayProbe"), w!("0"), ini) } == 0
    {
        host.log("Could not clear the one-shot overlay probe setting.");
    }
    if !host.engine_streamline {
        let mut fg: FgSetup = unsafe { std::mem::zeroed() };
        fg.struct_size = size_of::<FgSetup>() as u32;
        fg.backend = RSF_FG_BACKEND_AUTO;
        fg.max_generated_frames = u32::MAX;
        fg.runtime_directory_utf8 = host.fsr.as_ptr();
        fg.fsr3_directory_utf8 = host.fsr.as_ptr();
        fg.fsr4_directory_utf8 = host.fsr.as_ptr();
        fg.xess_directory_utf8 = host.xess.as_ptr();
        fg.streamline_directory_utf8 = host.streamline.as_ptr();
        fg.options = FgOptions {
            struct_size: size_of::<FgOptions>() as u32,
            mode: RSF_FG_OFF,
            generated_frames: 1,
            reflex_mode: RSF_REFLEX_ON,
            frame_limit_us: 0,
            debug: 0,
        };
        fg.depth_inverted = setup.depth_inverted;
        fg.units_to_meters = setup.units_to_meters;
        fg.runtime_switching = 1;
        fg.ui_mode = setup.ui_mode;
        fg.engine_type = host.engine_type;
        fg.engine_version_utf8 = host.engine_version.as_ptr();
        fg.project_id_utf8 = host.project_id.as_ptr();
        fg.log = Some(backend_log);
        fg.user = host.user();
        fg.accept_window = Some(accept_window);
        let installed = unsafe { (p.fg_install)(&fg) } == BACKEND_OK;
        host.fg_installed.store(installed, Ordering::Release);
    } else {
        let observed = ObserverSetup {
            struct_size: size_of::<ObserverSetup>() as u32,
            abi_version: 1,
            user: host.user(),
            accept_window: Some(accept_window),
            submit: Some(observer_submit),
            present: Some(present),
            swapchain: Some(observer_chain),
        };
        let mut observer = null_mut();
        if unsafe { (p.observer_create)(&observed, &mut observer) } != 0 {
            return NOT_READY;
        }
        host.observer.store(observer, Ordering::Release);
    }
    if host.fg_installed() && !host.capture_directory.is_empty() {
        let _ = unsafe { (p.fg_capture_outputs)(host.capture_directory.as_ptr()) };
    }
    host.log(if host.engine_streamline {
        "Engine presentation retained; SR borrows its existing graphics runtime."
    } else if host.fg_installed() {
        "Shared FG presentation installed before graphics; generation starts Off."
    } else {
        "FG presentation interception refused; native presentation retained."
    });
    OK
}

unsafe fn create(setup: *const Setup, services: *mut Services, p: &'static Platform) -> i32 {
    if setup.is_null() || unsafe { setup.cast::<u32>().read() } < size_of::<Setup>() as u32 {
        return INVALID;
    }
    let s = unsafe { &*setup };
    if s.configuration_path.is_null()
        || s.runtime_directory.is_null()
        || s.configuration_section.is_null()
        || unsafe { *s.configuration_section } == 0
        || s.engine_version_utf8.is_null()
        || s.presentation_owner > ENGINE_PRESENTATION
        || s.sr_backend_choices == 0
        || services.is_null()
        || unsafe { services.cast::<u32>().read() } < size_of::<Services>() as u32
        || unsafe { (*services).session_id } == 0
    {
        return INVALID;
    }
    if s.abi_version != HOST_ABI_VERSION
        || unsafe { (*services).abi_version } != RSF_GAME_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    // Install may invoke the plugin's logger before the frontend is published.
    // A concurrent or reentrant create must stop before installing anything.
    let Ok(_creation) = CREATION.try_lock() else {
        return BUSY;
    };
    if !FRONTEND.load(Ordering::Acquire).is_null() {
        return BUSY;
    }
    let services = unsafe { &mut *services };
    let host = crate::allocate_handle(unsafe { describe(s, services, p) });
    if host.is_null() {
        return NOT_READY;
    }
    // Diagnostics and observer hooks may already name the host when installation is refused,
    // so a refused host is retained rather than freed.
    let result = install(unsafe { &*host }, s);
    if result != OK {
        return result;
    }
    services.process_upscale = Some(process);
    services.submit = Some(submit);
    services.present = Some(present);
    services.render_pass = Some(render_pass);
    services.cpu_event = Some(cpu_event);
    services.process_latency = Some(process_latency);
    services.render_feedback = Some(render_feedback);
    services.user = host.cast();
    // The existing log callback keeps its original user argument through the runtime owner.
    services.log = Some(backend_log);
    FRONTEND.store(host, Ordering::Release);
    OK
}

// Entries behind the exported forwarders in render_host_platform.cpp.

/// # Safety
/// `setup` and `services` follow render_host.h; `platform` is the caller's static table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_host_create(
    setup: *const Setup,
    services: *mut Services,
    platform: *const Platform,
) -> i32 {
    // The platform table is a static in the native library that called this entry.
    let Some(platform): Option<&'static Platform> = (unsafe { platform.as_ref() }) else {
        return INVALID;
    };
    assert_eq!(platform.struct_size as usize, size_of::<Platform>());
    unsafe { create(setup, services, platform) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_host_graphics(user: *mut c_void, queue: Handle) -> i32 {
    let Some(host) = published(user) else {
        return INVALID;
    };
    if !host.engine_streamline || queue.is_null() || unsafe { (host.p.queue_direct)(queue) } == 0 {
        return INVALID;
    }
    host.lock().overlay_queue = retain(queue);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_host_device(user: *mut c_void, device: Handle) -> i32 {
    match published(user) {
        Some(host) => match host.observer.load(Ordering::Acquire) {
            observer if !observer.is_null() => unsafe {
                (host.p.observer_device)(observer, device)
            },
            _ => NOT_READY,
        },
        None => NOT_READY,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_host_factory(user: *mut c_void, factory: Handle) -> i32 {
    match published(user) {
        Some(host) => match host.observer.load(Ordering::Acquire) {
            observer if !observer.is_null() => unsafe {
                (host.p.observer_factory)(observer, factory)
            },
            _ => NOT_READY,
        },
        None => NOT_READY,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_render_host_graphics_ready(user: *mut c_void) -> u32 {
    published(user).map_or(0, |host| {
        u32::from(host.graphics_ready.load(Ordering::Acquire))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_host_native_object(
    user: *mut c_void,
    object: Handle,
) -> Handle {
    match published(user) {
        Some(host) if !object.is_null() => unsafe { (host.p.native_object)(object) },
        _ => null_mut(),
    }
}

/// # Safety
/// `output` is null or readable for its leading `struct_size`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_status_get(output: *mut Status) -> i32 {
    if output.is_null() || unsafe { output.cast::<u32>().read() } < size_of::<Status>() as u32 {
        return INVALID;
    }
    if unsafe { (*output).abi_version } != RSF_RENDER_FRONTEND_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Some(host) = frontend() else {
        return NOT_READY;
    };
    match host.status_get() {
        Ok(status) => {
            unsafe { output.write(status) };
            OK
        }
        Err(result) => result,
    }
}

/// # Safety
/// `output` is null or readable for its leading `struct_size`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_settings_get(output: *mut Settings) -> i32 {
    if output.is_null() || unsafe { output.cast::<u32>().read() } < size_of::<Settings>() as u32 {
        return INVALID;
    }
    if unsafe { (*output).abi_version } != RSF_RENDER_FRONTEND_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Some(host) = frontend() else {
        return NOT_READY;
    };
    match host.settings_get() {
        Ok(settings) => {
            unsafe { output.write(settings) };
            OK
        }
        Err(result) => result,
    }
}

/// # Safety
/// `request` is null or readable for its leading `struct_size`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_render_settings_apply(request: *const Settings) -> i32 {
    if request.is_null() || unsafe { request.cast::<u32>().read() } < size_of::<Settings>() as u32 {
        return INVALID;
    }
    let request = unsafe { &*request };
    if request.abi_version != RSF_RENDER_FRONTEND_ABI_VERSION {
        return ABI_MISMATCH;
    }
    match frontend() {
        Some(host) => host.settings_apply(request),
        None => NOT_READY,
    }
}
