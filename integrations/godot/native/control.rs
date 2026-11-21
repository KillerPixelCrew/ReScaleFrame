// SPDX-License-Identifier: MIT
//! Godot Runtime control, settings and session state formerly held by `runtime.cpp`: the
//! requested settings and their generation, the settings refusal rules, the engine-module
//! service routing, the D3D12 session (identity, paths, log, FG presenter install values and
//! boot state) and the status record with its assembly from Runtime facts.
//!
//! The Vulkan session (`vulkan.rs`) holds its own settings, generation and status record, as
//! `vulkan.cpp` did; while it is booted, configuration, settings and status route to it after the
//! same refusal rules. The D3D12 SR context, fences and capture stay C++ leaves in `runtime.cpp`
//! and report each evaluated pass here.
use crate::platform::{
    EngineServices, FgFacts, FgInstall, FgOptions, InputFacts, LatencyFacts, Paths, Platform,
    SETTINGS_ABI_VERSION, Settings, Setup, Sharpening, SrFrame, Status, WindowFacts,
};
use crate::vulkan::Vulkan;
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const BACKEND_OK: i32 = 0;
const SR_DLSS: u32 = 1;
const SR_SPATIAL: u32 = 6;
const SR_AUTO: u32 = 7;
const FG_BACKEND_AUTO: u32 = 7;
const FG_OFF: u32 = 0;
const FG_FIXED: u32 = 1;
const REFLEX_OFF: u32 = 0;
const REFLEX_BOOST: u32 = 2;
const QUALITY_ULTRA_QUALITY: u32 = 5;
const MAX_GENERATED_FRAMES: u32 = 15;
const FG_STAT_TOTAL_PRESENTED: u32 = 1;
const UI_MODE_BACKBUFFER_HUDLESS: u32 = 4;
const DLSS_ENGINE_CUSTOM: u32 = 0;
/// The D3D12 status record has always carried ABI 1, and `rsfg_status` copies it to the caller.
const D3D12_STATUS_RECORD_ABI: u32 = 1;

/// The settings both backends start from and the extension composes over.
pub(crate) const fn default_settings() -> Settings {
    Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: SETTINGS_ABI_VERSION,
        sr_backend: SR_AUTO,
        quality: 1,
        dlss_preset: 0,
        fg_backend: FG_BACKEND_AUTO,
        fg_mode: FG_OFF,
        generated_frames: 1,
        latency_mode: REFLEX_OFF,
        frame_limit_us: 0,
        gameplay: 1,
        sharpening: Sharpening {
            struct_size: 0,
            abi_version: 0,
            mode: 0,
            sharpness: 0.0,
            mas: 0,
            motion_sharpness: 0.0,
            motion_threshold: 0.0,
            motion_scale_limit: 0.0,
            contrast_enabled: 0,
            contrast: 0.0,
            depth_scale: 0.0,
            depth_bias: 0.0,
            clamp_output: 0,
            debug: 0,
        },
    }
}

/// The refusal rules of `rsfg_configure`, applied before either backend sees the settings.
pub(crate) fn settings_valid(platform: &Platform, value: &Settings) -> bool {
    value.sr_backend <= SR_AUTO
        && value.quality <= QUALITY_ULTRA_QUALITY
        && platform.dlss_preset_valid(value.dlss_preset)
        && (value.sharpening.struct_size == 0 || platform.sharpening_valid(&value.sharpening))
        && value.fg_mode <= FG_FIXED
        && value.latency_mode <= REFLEX_BOOST
        && (1..=MAX_GENERATED_FRAMES).contains(&value.generated_frames)
        && value.gameplay <= 1
        && (platform.fg_backend_known(value.fg_backend) || value.fg_backend == FG_BACKEND_AUTO)
        && !(value.sr_backend == SR_DLSS && value.quality == QUALITY_ULTRA_QUALITY)
}

/// Owner-held UTF-8 paths. Each boot attempt leaks one set so a Runtime that kept the previous
/// pointers never sees them freed.
struct SessionPaths {
    runtime: CString,
    streamline: CString,
    interposer: CString,
    fidelityfx: CString,
    xess: CString,
    version: CString,
    project: CString,
}

impl SessionPaths {
    fn new(runtime: &CStr, version: &CStr, project: &CStr) -> Option<Self> {
        let join = |suffix: &str| {
            let mut bytes = runtime.to_bytes().to_vec();
            bytes.extend_from_slice(suffix.as_bytes());
            CString::new(bytes).ok()
        };
        Some(Self {
            runtime: runtime.to_owned(),
            streamline: join("/streamline")?,
            interposer: join("/streamline/sl.interposer.dll")?,
            fidelityfx: join("/fidelityfx")?,
            xess: join("/xess")?,
            version: version.to_owned(),
            project: project.to_owned(),
        })
    }

    fn native(&self) -> Paths {
        Paths {
            runtime_directory: self.runtime.as_ptr(),
            streamline_directory: self.streamline.as_ptr(),
            streamline_interposer: self.interposer.as_ptr(),
            fidelityfx_directory: self.fidelityfx.as_ptr(),
            xess_directory: self.xess.as_ptr(),
            engine_version: self.version.as_ptr(),
            project_id: self.project.as_ptr(),
        }
    }

    /// The explicit-presentation FG host: Auto backend, unbounded generation, runtime switching,
    /// HUD-less backbuffer UI, inverted finite depth in metres and Godot's custom engine identity.
    fn fg_install(&self) -> FgInstall {
        FgInstall {
            backend: FG_BACKEND_AUTO,
            max_generated_frames: u32::MAX,
            runtime_switching: 1,
            ui_mode: UI_MODE_BACKBUFFER_HUDLESS,
            options: FgOptions {
                mode: FG_OFF,
                generated_frames: 1,
                reflex_mode: REFLEX_OFF,
                frame_limit_us: 0,
                debug: 0,
            },
            depth_inverted: 1,
            depth_infinite: 0,
            units_to_meters: 1.0,
            engine_type: DLSS_ENGINE_CUSTOM,
            fidelityfx_directory: self.fidelityfx.as_ptr(),
            xess_directory: self.xess.as_ptr(),
            streamline_directory: self.streamline.as_ptr(),
            engine_version: self.version.as_ptr(),
            project_id: self.project.as_ptr(),
        }
    }
}

struct Session {
    paths: Option<&'static SessionPaths>,
    log: rsf_game_log_fn,
    // The engine module's log user, an opaque pointer kept as an address.
    log_user: usize,
    settings: Settings,
    generation: u64,
    status: Status,
    services: Option<EngineServices>,
}

pub(crate) struct Control {
    booted: AtomicBool,
    boot_gate: Mutex<()>,
    session: Mutex<Session>,
    pub(crate) vulkan: Vulkan,
}

impl Control {
    pub(crate) fn new() -> Self {
        Self {
            vulkan: Vulkan::new(),
            booted: AtomicBool::new(false),
            boot_gate: Mutex::new(()),
            session: Mutex::new(Session {
                paths: None,
                log: None,
                log_user: 0,
                settings: default_settings(),
                generation: 1,
                status: Status {
                    struct_size: size_of::<Status>() as u32,
                    abi_version: D3D12_STATUS_RECORD_ABI,
                    ..Status::default()
                },
                services: None,
            }),
        }
    }

    // No leaf or engine callback runs while this lock is held: the log, the FG install and the
    // engine services can re-enter this owner.
    fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn booted(&self) -> bool {
        self.booted.load(Ordering::Acquire)
    }

    /// Any backend that accepts engine control.
    fn active(&self) -> bool {
        self.booted() || self.vulkan.booted()
    }

    /// `services` passed the size and ABI checks.
    pub(crate) fn attach_engine(&self, services: &EngineServices) -> bool {
        if services.configure.is_none() || services.reset_history.is_none() {
            return false;
        }
        self.session().services = Some(*services);
        true
    }

    /// Forwards to the engine module, which applies the request to its primary viewport and
    /// returns to `rsfg_configure`. `settings` passes through unchecked; the module checks it.
    pub(crate) fn configure_engine(&self, settings: *const Settings, viewport: u64) -> c_int {
        let configure = self
            .session()
            .services
            .and_then(|services| services.configure);
        match configure {
            // SAFETY: the engine module's service lives while its library is loaded.
            Some(configure) if self.active() => unsafe { configure(settings, viewport) },
            _ => 0,
        }
    }

    pub(crate) fn reset_history(&self) {
        let reset = self
            .session()
            .services
            .and_then(|services| services.reset_history);
        if let Some(reset) = reset
            && self.active()
        {
            // SAFETY: as in configure_engine.
            unsafe { reset() };
        }
    }

    /// The D3D12 session. `setup` passed the size and ABI checks; its strings are borrowed for
    /// the call. A booted session stays booted for the process.
    pub(crate) fn boot(&self, platform: &Platform, setup: &Setup) -> c_int {
        if setup.runtime_directory.is_null()
            || setup.engine_version.is_null()
            || setup.project_id.is_null()
        {
            return 0;
        }
        if self.booted() {
            return 1;
        }
        // FG installation can call into the host. Refuse an overlapping or reentrant boot.
        let Ok(_boot) = self.boot_gate.try_lock() else {
            return c_int::from(self.booted());
        };
        if self.booted() {
            return 1;
        }
        if !platform.prepare_capture() {
            return 0;
        }
        // SAFETY: non-null NUL-terminated strings borrowed for the call.
        let (runtime, version, project) = unsafe {
            (
                CStr::from_ptr(setup.runtime_directory),
                CStr::from_ptr(setup.engine_version),
                CStr::from_ptr(setup.project_id),
            )
        };
        let Some(paths) = SessionPaths::new(runtime, version, project) else {
            return 0;
        };
        let paths: &'static SessionPaths = Box::leak(Box::new(paths));
        let session_id = platform.tick_count().wrapping_add(1);
        {
            let mut session = self.session();
            session.paths = Some(paths);
            session.log = setup.log;
            session.log_user = setup.log_user as usize;
            session.status.session_id = session_id;
        }
        let result = platform.fg_install(&paths.fg_install());
        self.session().status.fg_result = result;
        let booted = result == BACKEND_OK;
        self.booted.store(booted, Ordering::Release);
        c_int::from(booted)
    }

    /// `settings` passed the size and ABI checks.
    pub(crate) fn configure(&self, platform: &Platform, settings: &Settings) -> c_int {
        if !settings_valid(platform, settings) {
            return 0;
        }
        if self.vulkan.booted() {
            return self.vulkan.configure(platform, settings);
        }
        {
            let mut session = self.session();
            let current = &session.settings;
            if current.sr_backend != settings.sr_backend
                || current.quality != settings.quality
                || current.dlss_preset != settings.dlss_preset
            {
                session.generation = session.generation.wrapping_add(1);
            }
            session.settings = *settings;
        }
        platform.fg_options(&FgOptions {
            mode: settings.fg_mode,
            generated_frames: settings.generated_frames,
            reflex_mode: settings.latency_mode,
            frame_limit_us: settings.frame_limit_us,
            debug: 0,
        });
        1
    }

    /// # Safety
    /// `output` passed the size and ABI checks and is writable for `rsf_godot_settings`.
    pub(crate) unsafe fn settings(&self, output: *mut Settings) -> c_int {
        let (settings, result) = if self.vulkan.booted() {
            self.vulkan.settings()
        } else {
            (self.session().settings, 1)
        };
        // SAFETY: the caller's guarantee.
        unsafe { output.write(settings) };
        result
    }

    pub(crate) fn snapshot(&self) -> (Settings, u64) {
        let session = self.session();
        (session.settings, session.generation)
    }

    /// # Safety
    /// `output` passed the size and ABI checks and is writable for `rsf_godot_status`.
    pub(crate) unsafe fn status(&self, platform: &Platform, output: *mut Status) -> c_int {
        if self.vulkan.booted() {
            let (mut value, result) = self.vulkan.status(platform);
            if result != 0 {
                apply_inputs(&mut value, &platform.cpu_counters());
                apply_window(&mut value, &platform.window_facts());
            }
            // SAFETY: the caller's guarantee.
            unsafe { output.write(value) };
            return result;
        }
        if !self.booted() {
            return 0;
        }
        let mut value = self.session().status;
        apply_fg(&mut value, &platform.fg_facts());
        apply_window(&mut value, &platform.window_facts());
        if let Some(latency) = platform.latency_facts() {
            apply_latency(&mut value, &latency);
        }
        // SAFETY: the caller's guarantee.
        unsafe { output.write(value) };
        1
    }

    pub(crate) fn session_id(&self) -> u64 {
        self.session().status.session_id
    }

    pub(crate) fn paths(&self) -> Option<Paths> {
        self.session().paths.map(SessionPaths::native)
    }

    /// One evaluated D3D12 SR pass. FSR1 is evaluated by Godot before the callback; the native
    /// context only normalises its depth and motion, so a spatial request reports spatial.
    pub(crate) fn sr_frame(&self, frame: &SrFrame) {
        let evaluated = frame.evaluated == BACKEND_OK;
        let mut session = self.session();
        let status = &mut session.status;
        status.requested_sr = frame.requested_sr;
        status.active_sr = match (evaluated, frame.requested_sr) {
            (false, _) => 0,
            (true, SR_SPATIAL) => SR_SPATIAL,
            (true, _) => frame.context_backend,
        };
        status.sr_result = if frame.context_result != BACKEND_OK {
            frame.context_result
        } else {
            frame.evaluated
        };
        status.render_width = frame.render_width;
        status.render_height = frame.render_height;
        status.output_width = frame.output_width;
        status.output_height = frame.output_height;
        if evaluated {
            status.evaluated_frames = status.evaluated_frames.wrapping_add(1);
        } else {
            status.refused_frames = status.refused_frames.wrapping_add(1);
        }
    }

    /// Present found no SR pass for the frame it shows.
    pub(crate) fn sr_missed(&self) {
        self.session().status.active_sr = 0;
    }

    pub(crate) fn log(&self, message: *const c_char) {
        let (log, user) = {
            let session = self.session();
            (session.log, session.log_user)
        };
        if let Some(log) = log {
            // SAFETY: the engine module's logger lives while its library is loaded; the message
            // is the caller's borrow.
            unsafe { log(user as *mut c_void, message) };
        }
    }
}

fn apply_inputs(status: &mut Status, inputs: &InputFacts) {
    status.keyboard_inputs = inputs.keyboard;
    status.mouse_inputs = inputs.mouse;
    status.controller_inputs = inputs.controller;
    status.pcl_pings = inputs.pings;
    status.input_substitutes = inputs.substitutes;
    status.order_violations = inputs.order_violations;
    status.derived_boundaries = inputs.derived_boundaries;
}

fn apply_window(status: &mut Status, window: &WindowFacts) {
    status.foreground_process_id = window.foreground_process_id;
    status.window_foreground = u32::from(window.foreground_process_id == window.current_process_id);
    status.window_visible = window.input_window_visible;
}

/// The presentation provider's switch failure outranks the FG session's last result.
fn apply_fg(status: &mut Status, fg: &FgFacts) {
    status.fg_backend = fg.backend;
    status.fg_available = fg.available;
    status.fg_active = fg.active;
    status.fg_result = if fg.switch_result != BACKEND_OK {
        fg.switch_result
    } else {
        fg.last_result
    };
    status.reason = fg.reason;
    status.max_generated_frames = fg.max_generated_frames;
    status.generated_frames = fg.generated_frames;
    status.latency_available = fg.latency_available;
    status.latency_mode = fg.latency_mode;
    status.tagged_frames = fg.tagged_frames;
    status.total_presented = fg.total_presented;
    status.generated_presented = fg.generated_presented;
    status.presentation_statistics_valid =
        u32::from(fg.statistics_flags & FG_STAT_TOTAL_PRESENTED != 0);
    status.presentation_statistics_flags = fg.statistics_flags;
    status.source_frame_id = fg.source_frame_id;
    status.provider_generation = fg.provider_generation;
    status.generated_callbacks = fg.generated_callbacks;
}

fn apply_latency(status: &mut Status, latency: &LatencyFacts) {
    apply_inputs(status, &latency.inputs);
    status.sleep_calls = latency.paced_frames;
    status.marker_calls = latency.marker_calls;
    status.pcl_available = latency.pcl_available;
    status.latency_report_available = latency.report_available;
    status.latency_owner = latency.owner;
}

#[cfg(test)]
mod tests;
