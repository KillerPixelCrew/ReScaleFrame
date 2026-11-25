// SPDX-License-Identifier: MIT
//! The Unity package behind ReScaleFrame.Unity.Native.dll's rsfu_* exports
//! (integrations/unity/native/package.c). The package is an integration: it prepares the engine's
//! inputs (its D3D12, D3D11 and Vulkan renderer bridges), presents the output (the Editor preview)
//! and reports its engine's facts (the gameplay screen class, received input, CPU frames). Every
//! technique is the Runtime's D3D12 host, which each session attaches
//! (`rsf_callback_host_start` with the V3 setup): SR contexts per view, the plan, settings, the
//! provider requests and the shared overlay.
//!
//! This module keeps the developer's settings API as a translation onto that host:
//! rsfu_configure and rsfu_settings are the Runtime's `rsf_render_settings`, so the developer's
//! menu and the overlay read and write one store, and rsfu_status merges the host's SR status
//! with the Runtime's generation and latency status. It also owns the session lifecycle with its
//! result codes, the generation install (presentation.rs), the session services: the log, the
//! input window and the routing of CPU events, window messages and rsfu_input (services.rs), and
//! the render callback's routing, the generation window guard and Unity's device lifecycle
//! (routing.rs). Rust serializes the session exports and Unity device events with OPERATIONS.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
mod native;
mod presentation;
mod routing;
pub(crate) mod services;
#[cfg(windows)]
mod windows;

use self::presentation::{Identity, Presentation};
use self::services::Services;
use crate::sys::{
    RSF_FG_BACKEND_AUTO, RSF_FG_STAT_TOTAL_PRESENTED, RSF_RENDER_FRONTEND_ABI_VERSION, RSF_SR_AUTO,
    RSF_SR_DLSS, RSF_SR_NONE, RSF_SR_XESS, RSF_UNITY_SETTINGS_ABI_VERSION,
    rsf_callback_host_status, rsf_native_fg_status, rsf_native_latency_status, rsf_render_settings,
    rsf_sharpening_options, rsf_unity_frame_counters, rsf_unity_native_api,
    rsf_unity_package_settings, rsf_unity_package_status,
};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{backend_result, rsf_game_host_services};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU8, AtomicU32, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard};

/// rsfu_initialize's results, part of the managed contract (ReScaleFrameRuntime.Apply).
mod code {
    pub(super) const FAILED: i32 = 0;
    pub(super) const STARTED: i32 = 1;
    /// The previous session still owns GPU work or Runtime tokens.
    pub(super) const BUSY: i32 = 2;
}
/// The status and frame-counter versions, and the latency status request version.
const STATUS_ABI_VERSION: u32 = 1;
const FRAME_COUNTERS_ABI_VERSION: u32 = 1;
const LATENCY_STATUS_VERSION: u32 = 1;

/// The developer's settings before the first rsfu_configure, which are also the host's start
/// values for keys the preferences never saved. A zero sharpening struct_size selects the
/// Runtime's sharpening defaults.
const DEFAULT_SETTINGS: rsf_unity_package_settings = rsf_unity_package_settings {
    struct_size: size_of::<rsf_unity_package_settings>() as u32,
    abi_version: RSF_UNITY_SETTINGS_ABI_VERSION,
    sr_backend: RSF_SR_AUTO,
    quality: 1,
    dlss_preset: 0,
    fg_backend: RSF_FG_BACKEND_AUTO,
    fg_mode: 0,
    generated_frames: 1,
    latency_mode: 0,
    frame_limit_us: 0,
    gameplay: 1,
    sharpening: rsf_sharpening_options {
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
};

const fn fresh_status() -> rsf_unity_package_status {
    // SAFETY: every field is an integer, for which zero is valid.
    let mut status: rsf_unity_package_status = unsafe { std::mem::zeroed() };
    status.struct_size = size_of::<rsf_unity_package_status>() as u32;
    status.abi_version = STATUS_ABI_VERSION;
    status
}

static OPERATIONS: Mutex<()> = Mutex::new(());
static UNITY_INTERFACES: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static UNITY_GRAPHICS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

// The Runtime, bridge and Win32 calls: native.rs's bindings in production, the test module's
// fakes under test. SAFETY for every `raw` call: the leaves borrow their pointer arguments for
// the call only.
#[cfg(not(test))]
use native::raw;
#[cfg(test)]
use tests as raw;

/// The Runtime's generation status; zero-filled apart from struct_size when it has none.
fn fg_status() -> rsf_native_fg_status {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    unsafe { raw::fg_status(&mut status) };
    status
}

fn latency_status() -> Option<rsf_native_latency_status> {
    let mut status = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: LATENCY_STATUS_VERSION,
        ..Default::default()
    };
    (unsafe { raw::latency_status(&mut status) } != 0).then_some(status)
}

/// The attached host's SR status; None without a session.
fn host_status() -> Option<rsf_callback_host_status> {
    let mut status = rsf_callback_host_status {
        struct_size: size_of::<rsf_callback_host_status>() as u32,
        ..Default::default()
    };
    (unsafe { raw::host_status(&mut status) } != 0).then_some(status)
}

/// The attached host's settings; None without a session.
fn host_settings() -> Option<rsf_render_settings> {
    let mut settings = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    (unsafe { raw::settings_get(&mut settings) } == 0).then_some(settings)
}

/// The developer's request as the Runtime's settings over `saved`: no SR backend is SR off with
/// the saved backend kept, and a zero sharpening size is the Runtime's defaults.
fn render_settings(
    input: &rsf_unity_package_settings,
    saved: &rsf_render_settings,
) -> rsf_render_settings {
    let o = if input.sharpening.struct_size != 0 {
        input.sharpening
    } else {
        raw::sharpening_defaults()
    };
    rsf_render_settings {
        sr_enabled: u32::from(input.sr_backend != RSF_SR_NONE),
        sr_backend: if input.sr_backend == RSF_SR_NONE {
            saved.sr_backend
        } else {
            input.sr_backend
        },
        sr_quality: input.quality,
        dlss_preset: input.dlss_preset,
        fg_enabled: input.fg_mode,
        fg_backend: input.fg_backend,
        generated_frames: input.generated_frames,
        latency: input.latency_mode,
        frame_limit_us: input.frame_limit_us,
        sharpening_mode: o.mode,
        sharpening_sharpness: o.sharpness,
        sharpening_mas: o.mas,
        sharpening_motion_sharpness: o.motion_sharpness,
        sharpening_motion_threshold: o.motion_threshold,
        sharpening_motion_scale_limit: o.motion_scale_limit,
        sharpening_contrast_enabled: o.contrast_enabled,
        sharpening_contrast: o.contrast,
        sharpening_depth_scale: o.depth_scale,
        sharpening_depth_bias: o.depth_bias,
        sharpening_clamp_output: o.clamp_output,
        sharpening_debug: o.debug,
        ..*saved
    }
}

/// The Runtime's settings as the developer's API reads them. The gameplay flag is the
/// developer's own, and so is a zero sharpening size, which keeps meaning the defaults.
fn package_settings(
    settings: &rsf_render_settings,
    developer: &rsf_unity_package_settings,
) -> rsf_unity_package_settings {
    let sharpening = if developer.sharpening.struct_size == 0 {
        developer.sharpening
    } else {
        rsf_sharpening_options {
            mode: settings.sharpening_mode,
            sharpness: settings.sharpening_sharpness,
            mas: settings.sharpening_mas,
            motion_sharpness: settings.sharpening_motion_sharpness,
            motion_threshold: settings.sharpening_motion_threshold,
            motion_scale_limit: settings.sharpening_motion_scale_limit,
            contrast_enabled: settings.sharpening_contrast_enabled,
            contrast: settings.sharpening_contrast,
            depth_scale: settings.sharpening_depth_scale,
            depth_bias: settings.sharpening_depth_bias,
            clamp_output: settings.sharpening_clamp_output,
            debug: settings.sharpening_debug,
            ..developer.sharpening
        }
    };
    rsf_unity_package_settings {
        sr_backend: if settings.sr_enabled == 0 {
            RSF_SR_NONE
        } else {
            settings.sr_backend
        },
        quality: settings.sr_quality,
        dlss_preset: settings.dlss_preset,
        fg_backend: settings.fg_backend,
        fg_mode: settings.fg_enabled,
        generated_frames: settings.generated_frames,
        latency_mode: settings.latency,
        frame_limit_us: settings.frame_limit_us,
        sharpening,
        ..*developer
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Session {
    Unopened,
    Stopped,
    /// Open can acquire Vulkan objects before it returns a session ID.
    Opening,
    Started(u64),
}

/// The attached host's services, which the render callback forwards to.
#[derive(Clone, Copy)]
struct Host(rsf_game_host_services);
// SAFETY: the Runtime's process-lifetime callbacks and its host address, never dereferenced here.
unsafe impl Send for Host {}

struct State {
    /// The developer's last request, kept for the gameplay flag and for a session to come.
    settings: rsf_unity_package_settings,
    /// A request made without a session, applied by the next session's host.
    pending: bool,
    session: Session,
}

struct Package {
    state: Mutex<State>,
    host: Mutex<Option<Host>>,
    presentation: Presentation,
    services: Services,
    /// A renderer bridge failure, reported as the SR result until the next session.
    bridge_result: AtomicI32,
    /// Unity's device (routing.rs), written only under the package operation lock.
    device: AtomicU8,
    /// Editor preview refusals counted toward the process's diagnostic budget.
    preview_reports: AtomicU32,
}

static PACKAGE: Package = Package::new();

impl Package {
    const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                settings: DEFAULT_SETTINGS,
                pending: false,
                session: Session::Unopened,
            }),
            host: Mutex::new(None),
            presentation: Presentation::new(),
            services: Services::new(),
            bridge_result: AtomicI32::new(0),
            device: AtomicU8::new(routing::DEVICE_WAITING),
            preview_reports: AtomicU32::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock_or_recover(&self.state)
    }

    fn host(&self) -> Option<rsf_game_host_services> {
        lock_or_recover(&self.host).map(|host| host.0)
    }

    /// The developer's request: the host checks, applies and saves it as the same change made in
    /// the overlay. Without a session the Runtime still checks it, and an accepted request waits
    /// for the next session's host.
    fn configure(&self, input: &rsf_unity_package_settings) -> bool {
        const NOT_READY: i32 = backend_result::NOT_READY;
        if input.abi_version != RSF_UNITY_SETTINGS_ABI_VERSION {
            return false;
        }
        let saved = host_settings();
        let base = saved.unwrap_or(rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_backend: RSF_SR_AUTO,
            ..Default::default()
        });
        match unsafe { raw::settings_apply(&render_settings(input, &base)) } {
            backend_result::OK => {}
            NOT_READY if saved.is_none() => self.lock().pending = true,
            _ => return false,
        }
        self.lock().settings = *input;
        true
    }
    /// The host's settings in the developer's form; the last request without a session.
    fn settings(&self) -> rsf_unity_package_settings {
        let developer = self.lock().settings;
        host_settings().map_or(developer, |saved| package_settings(&saved, &developer))
    }

    /// The host's SR status merged with the Runtime's generation and latency status, read in
    /// order: generation status, present backend, provider-switch result, latency status.
    fn status(&self) -> rsf_unity_package_status {
        let fg = fg_status();
        let fg_backend = unsafe { raw::present_backend() };
        let fg_result = rsf_game_sys::presentation_result::PresentationResult {
            session: fg.last_result,
            provider_switch: unsafe { raw::present_switch_result() },
        }
        .aggregate();
        let latency = latency_status().unwrap_or_default();
        let sr = host_status().unwrap_or_default();
        let mut status = fresh_status();
        status.requested_sr = self.settings().sr_backend;
        status.active_sr = sr.active_backend;
        (status.render_width, status.render_height) = (sr.render_width, sr.render_height);
        (status.output_width, status.output_height) = (sr.output_width, sr.output_height);
        status.sr_result = sr.result;
        status.evaluated_frames = sr.evaluated_frames;
        status.refused_frames = sr.refused_frames;
        let failure = self.bridge_result.load(SeqCst);
        if failure != 0 {
            status.sr_result = failure;
            status.active_sr = 0;
        }
        status.fg_backend = fg_backend;
        status.fg_available = fg.available;
        status.fg_active = fg.vendor.active;
        status.max_generated_frames = fg.vendor.max_generated_frames;
        status.generated_frames = fg.vendor.effective_generated_frames;
        status.reason = fg.reason;
        status.tagged_frames = fg.tagged_frames;
        status.fg_result = fg_result;
        status.latency_available = latency.available;
        status.latency_mode = latency.effective_mode;
        status.sleep_calls = latency.paced_frames;
        status.marker_calls = latency.marker_calls;
        status.pcl_available = latency.pcl_available;
        status.latency_report_available = latency.report_available;
        status.latency_owner = latency.owner;
        status.keyboard_inputs = latency.events.input_events[0];
        status.mouse_inputs = latency.events.input_events[1];
        status.controller_inputs = latency.events.input_events[2];
        status.pcl_pings = latency.events.pings;
        status.input_substitutes = latency.events.substitutes;
        status.order_violations = latency.events.order_violations;
        status.derived_boundaries = latency.derived_boundaries;
        status
    }

    /// A temporal SR plan is enabled, so the managed adapter jitters and records its inputs.
    fn temporal_ready(&self) -> bool {
        self.bridge_result.load(SeqCst) == 0
            && host_status().is_some_and(|sr| {
                sr.enabled != 0 && (RSF_SR_DLSS..=RSF_SR_XESS).contains(&sr.active_backend)
            })
    }

    fn frame_counters(&self) -> rsf_unity_frame_counters {
        let fg = fg_status();
        rsf_unity_frame_counters {
            struct_size: size_of::<rsf_unity_frame_counters>() as u32,
            abi_version: FRAME_COUNTERS_ABI_VERSION,
            valid: u32::from(fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0),
            active: fg.vendor.active,
            total_presented: fg.vendor.total_presented,
            generated_presented: fg.vendor.generated_presented,
            provider_generation: unsafe { raw::present_generation() },
        }
    }

    /// The session's identity replaces the loaded one before open, which may install from it
    /// and attaches the Runtime host. The bridge's managed API is returned once started.
    fn initialize(&self, identity: Identity) -> Result<rsf_unity_native_api, i32> {
        let session = self.lock().session;
        if matches!(session, Session::Opening | Session::Started(_)) && !self.retire() {
            return Err(code::BUSY);
        }
        self.bridge_result.store(0, SeqCst);
        self.lock().session = Session::Opening;
        self.presentation.set_identity(identity);
        if !self.open() {
            return Err(code::FAILED);
        }
        let (pending, settings) = {
            let mut state = self.lock();
            (std::mem::take(&mut state.pending), state.settings)
        };
        if pending && !self.configure(&settings) {
            self.log("The settings requested before the session were refused.");
        }
        self.lock().session = Session::Started(self.services.session());
        raw::bridge_activate(1);
        Ok(self.services.api())
    }

    /// Closes the bridge once its work is drained, ends generation's session, then stops the
    /// host, whose SR contexts the drained work no longer uses.
    fn retire(&self) -> bool {
        let session = self.lock().session;
        if session == Session::Stopped {
            return true;
        }
        if !raw::bridge_try_close() {
            return false;
        }
        // Native admission and the Vulkan presentation lease hold the checked fence proof.
        if let Session::Started(id) = session
            && unsafe { raw::end_session(id) } == 0
        {
            raw::bridge_finish_retirement(false);
            return false;
        }
        if lock_or_recover(&self.host).take().is_some() {
            let _ = unsafe { raw::host_stop() };
        }
        raw::bridge_finish_retirement(true);
        self.services.retired();
        self.lock().session = Session::Stopped;
        true
    }

    fn bridge_failed(&self, result: i32) {
        self.bridge_result.store(result, SeqCst);
    }
}

/// Only the size field is readable from a short structure.
unsafe fn struct_size<T>(value: *const T) -> u32 {
    unsafe { value.cast::<u32>().read() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_configure(input: *const rsf_unity_package_settings) -> c_int {
    let _operation = lock_or_recover(&OPERATIONS);
    if input.is_null()
        || unsafe { struct_size(input) } != size_of::<rsf_unity_package_settings>() as u32
    {
        return 0;
    }
    c_int::from(PACKAGE.configure(&unsafe { input.read() }))
}

/// Accepts a longer structure and writes only this version's fields.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_settings(output: *mut rsf_unity_package_settings) -> c_int {
    let _operation = lock_or_recover(&OPERATIONS);
    if output.is_null()
        || unsafe { struct_size(output) } < size_of::<rsf_unity_package_settings>() as u32
        || unsafe { (*output).abi_version } != RSF_UNITY_SETTINGS_ABI_VERSION
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.settings()) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_status(output: *mut rsf_unity_package_status) -> c_int {
    let _operation = lock_or_recover(&OPERATIONS);
    if output.is_null()
        || unsafe { struct_size(output) } != size_of::<rsf_unity_package_status>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.status()) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_frame_counters(output: *mut rsf_unity_frame_counters) -> c_int {
    if output.is_null()
        || unsafe { struct_size(output) } != size_of::<rsf_unity_frame_counters>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.frame_counters()) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsfu_temporal_ready() -> c_int {
    c_int::from(PACKAGE.temporal_ready())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_initialize(
    runtime: *const c_char,
    engine: *const c_char,
    project: *const c_char,
    output: *mut rsf_unity_native_api,
) -> c_int {
    rsf_game_sys::panic_record::install();
    let _operation = lock_or_recover(&OPERATIONS);
    if runtime.is_null() || engine.is_null() || project.is_null() || output.is_null() {
        return code::FAILED;
    }
    match PACKAGE.device.load(SeqCst) {
        routing::DEVICE_RETAINED => return code::FAILED,
        routing::DEVICE_WAITING => return code::BUSY,
        _ => {}
    }
    let text = |value: *const c_char| unsafe { CStr::from_ptr(value) }.to_bytes();
    let identity = Identity::new(text(runtime), text(engine), text(project));
    match PACKAGE.initialize(identity) {
        Ok(api) => {
            unsafe { output.write(api) };
            code::STARTED
        }
        Err(result) => result,
    }
}

pub(crate) fn rsf_unity_package_bridge_failed(result: i32) {
    PACKAGE.bridge_failed(result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::rsf_fg_status;
    use crate::sys::rsf_fg12_setup;
    use crate::sys::{RSF_SR_SPATIAL, rsf_callback_host_setup};
    use rsf_game_sys::{RSF_SCREEN_FLIGHT, rsf_game_cpu_event, rsf_game_render_config};
    use rsf_game_sys::{RSF_SCREEN_MENU, rsf_game_render_pass};
    use std::cell::{Cell, RefCell};
    use std::ffi::CStr;
    use std::ptr::without_provenance_mut;

    #[derive(Clone, Copy)]
    struct Fake {
        drained: bool,
        close_held: bool,
        ended: bool,
        /// The bridge accepts the session's host services.
        prepared: bool,
        /// The Vulkan surface, where there is one, was created.
        surface: bool,
        /// %LOCALAPPDATA%.
        local: Option<&'static str>,
        /// log_open's handle; 0 is a refusal.
        file: usize,
        /// The player's Unity window and the Editor preview's parent; 0 is none.
        player: usize,
        parent: usize,
        subclass_accepted: bool,
        /// A WM_INPUT header's device type; None when it cannot be read.
        raw: Option<u32>,
        last_result: i32,
        switch_result: i32,
        latency: bool,
        install_result: i32,
        /// The Runtime host attaches, and is attached.
        host_accepts: bool,
        attached: bool,
        /// The host's answer to a settings request.
        apply_result: i32,
        /// The host's SR status.
        sr: rsf_callback_host_status,
        /// The Editor preview presents, and is prepared, its window answers preview_owns.
        shown: bool,
        preview_ready: bool,
        preview_owns: bool,
        /// Unity renders with Vulkan; the window is the player's surface.
        vulkan: bool,
        surface_owns: bool,
        /// window_class's answer.
        class: u32,
        /// UnityPluginLoad stored Unity's interfaces.
        registry: bool,
    }

    const SERVING: rsf_callback_host_status = rsf_callback_host_status {
        struct_size: 48,
        requested_backend: RSF_SR_AUTO,
        active_backend: 3,
        enabled: 1,
        result: 0,
        render_width: 1280,
        render_height: 720,
        output_width: 1920,
        output_height: 1080,
        evaluated_frames: 12,
        refused_frames: 2,
    };

    const READY: Fake = Fake {
        drained: true,
        close_held: false,
        ended: true,
        prepared: true,
        surface: true,
        local: Some("L"),
        file: 0x10,
        player: 0x20,
        parent: 0x40,
        subclass_accepted: true,
        raw: Some(1),
        last_result: 0,
        switch_result: -7,
        latency: true,
        install_result: 0,
        host_accepts: true,
        attached: false,
        apply_result: 0,
        sr: SERVING,
        shown: true,
        preview_ready: true,
        preview_owns: true,
        vulkan: false,
        surface_owns: true,
        class: 1,
        registry: true,
    };
    /// The fake host's services user.
    const HOST_USER: usize = 0x77;

    thread_local! {
        static FAKE: Cell<Fake> = const { Cell::new(READY) };
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static SAVED: Cell<rsf_render_settings> = Cell::new(saved_defaults());
    }

    fn saved_defaults() -> rsf_render_settings {
        rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: 1,
            sr_backend: RSF_SR_AUTO,
            sr_quality: 1,
            fg_backend: RSF_FG_BACKEND_AUTO,
            generated_frames: 1,
            ..Default::default()
        }
    }

    fn fake() -> Fake {
        FAKE.with(Cell::get)
    }
    fn set(change: impl FnOnce(&mut Fake)) {
        let mut value = fake();
        change(&mut value);
        FAKE.with(|cell| cell.set(value));
    }
    fn call(name: String) {
        CALLS.with(|calls| calls.borrow_mut().push(name));
    }
    fn calls() -> Vec<String> {
        CALLS.with(|calls| calls.borrow_mut().drain(..).collect())
    }
    fn wide(path: &[u16]) -> String {
        String::from_utf16(path).unwrap()
    }
    fn terminated_text(path: *const u16) -> String {
        let mut length = 0;
        while unsafe { *path.add(length) } != 0 {
            length += 1;
        }
        wide(unsafe { std::slice::from_raw_parts(path, length) })
    }
    fn handle(value: usize) -> *mut c_void {
        without_provenance_mut(value)
    }

    // The Runtime host, as far as the package reaches it.
    unsafe extern "C" fn host_render(
        user: *mut c_void,
        commands: *mut c_void,
        pass: *const rsf_game_render_pass,
        begin: u32,
    ) {
        assert_eq!(user as usize, HOST_USER);
        let p = unsafe { &*pass };
        call(format!(
            "host pass {:#x} {} {} {begin} {}",
            commands as usize, p.role, p.view_key, p.screen
        ));
    }
    unsafe extern "C" fn host_config(_: *mut c_void, _: *mut rsf_game_render_config) -> c_int {
        1
    }
    pub(super) unsafe fn host_start(setup: *const rsf_callback_host_setup) -> u32 {
        let v3 = unsafe { &*setup.cast::<crate::sys::rsf_callback_host_setup_v3>() };
        let s = &v3.base;
        assert_eq!((s.struct_size, s.abi_version), (size_of_val(v3) as u32, 3));
        assert!(s.plugin_path.is_null() && s.probe.is_null() && s.set_engine_spatial.is_some());
        assert_eq!((s.backend, s.quality, s.dlss_preset), (RSF_SR_AUTO, 1, 0));
        let text = |value: *const c_char| unsafe { CStr::from_ptr(value) }.to_str().unwrap();
        call(format!(
            "host start {} {} {} {} {}",
            terminated_text(s.log_path),
            terminated_text(s.preferences_path),
            text(s.streamline_directory_utf8),
            text(s.fsr3_directory_utf8),
            s.engine_type
        ));
        if !fake().host_accepts {
            return 10;
        }
        unsafe {
            v3.services.write(rsf_game_host_services {
                struct_size: size_of::<rsf_game_host_services>() as u32,
                abi_version: rsf_game_sys::RSF_GAME_ABI_VERSION,
                session_id: 42,
                user: handle(HOST_USER),
                render_pass: Some(host_render),
                render_config: Some(host_config),
                ..Default::default()
            })
        };
        set(|f| f.attached = true);
        0
    }
    pub(super) unsafe fn host_stop() -> u32 {
        call("host stop".into());
        set(|f| f.attached = false);
        0
    }
    pub(super) unsafe fn host_status(status: *mut rsf_callback_host_status) -> u32 {
        if !fake().attached {
            return 0;
        }
        unsafe { status.write(fake().sr) };
        1
    }
    pub(super) unsafe fn settings_get(output: *mut rsf_render_settings) -> i32 {
        if !fake().attached {
            return -3;
        }
        unsafe { output.write(SAVED.with(Cell::get)) };
        0
    }
    pub(super) unsafe fn settings_apply(request: *const rsf_render_settings) -> i32 {
        let r = unsafe { *request };
        call(format!(
            "apply sr={}/{} q={} preset={} fg={}/{} frames={} latency={} limit={} sharpness={}",
            r.sr_enabled,
            r.sr_backend,
            r.sr_quality,
            r.dlss_preset,
            r.fg_enabled,
            r.fg_backend,
            r.generated_frames,
            r.latency,
            r.frame_limit_us,
            r.sharpening_sharpness
        ));
        let result = fake().apply_result;
        if result != 0 {
            return result;
        }
        if !fake().attached {
            return backend_result::NOT_READY;
        }
        SAVED.with(|saved| saved.set(r));
        0
    }
    pub(super) extern "C" fn set_engine_spatial(_: u32, _: u32) -> c_int {
        1
    }
    pub(super) fn sharpening_defaults() -> rsf_sharpening_options {
        rsf_sharpening_options {
            struct_size: 56,
            sharpness: 0.5,
            ..Default::default()
        }
    }

    pub(super) unsafe fn fg_status(status: *mut rsf_native_fg_status) -> c_int {
        call("fg status".into());
        let status = unsafe { &mut *status };
        assert_eq!(status.struct_size, 352);
        status.available = 1;
        status.reason = 3;
        status.last_result = fake().last_result;
        status.tagged_frames = 9;
        status.vendor = rsf_fg_status {
            active: 1,
            max_generated_frames: 3,
            effective_generated_frames: 2,
            low_latency_available: 1,
            effective_reflex: 2,
            valid_statistics: RSF_FG_STAT_TOTAL_PRESENTED,
            total_presented: 40,
            generated_presented: 20,
            ..Default::default()
        };
        1
    }
    pub(super) unsafe fn latency_status(status: *mut rsf_native_latency_status) -> c_int {
        call("latency".into());
        let status = unsafe { &mut *status };
        assert_eq!((status.struct_size, status.version), (152, 1));
        if !fake().latency {
            return 0;
        }
        status.paced_frames = 11;
        status.available = 1;
        status.effective_mode = 1;
        status.owner = 2;
        status.events.input_events = [4, 5, 6];
        status.derived_boundaries = 8;
        1
    }
    pub(super) unsafe fn present_backend() -> u32 {
        call("backend".into());
        5
    }
    pub(super) unsafe fn present_switch_result() -> i32 {
        call("switch".into());
        fake().switch_result
    }
    pub(super) unsafe fn present_generation() -> u64 {
        call("generation".into());
        77
    }
    pub(super) fn bridge_activate(enabled: u32) {
        call(format!("activate {enabled}"));
    }
    pub(super) fn bridge_try_close() -> bool {
        assert!(!fake().close_held);
        call("close".into());
        set(|f| f.close_held = f.drained);
        fake().close_held
    }
    pub(super) unsafe fn end_session(session: u64) -> c_int {
        assert!(fake().close_held);
        call(format!("end {session}"));
        fake().ended.into()
    }
    pub(super) fn bridge_finish_retirement(commit: bool) {
        assert!(fake().close_held);
        set(|f| f.close_held = false);
        call(if commit { "release" } else { "retain" }.into());
    }
    pub(super) unsafe fn fg_install(setup: *const rsf_fg12_setup) -> i32 {
        let s = unsafe { *setup };
        let o = s.options;
        let text = |value: *const c_char| unsafe { CStr::from_ptr(value) }.to_str().unwrap();
        // The package owns both callbacks and passes a null user.
        let lent = s.accept_window.map(|f| f as usize)
            == Some(routing::accept_window as *const () as usize)
            && s.log.map(|f| f as usize) == Some(services::backend_log as *const () as usize)
            && s.user.is_null();
        assert!(lent);
        call(format!(
            "install {} {} {} {} | {} {} {} {} {} {} | {} {} {} {} {} | {} {} {} {} | {} {} {}",
            s.struct_size,
            s.backend,
            s.max_generated_frames,
            text(s.runtime_directory_utf8),
            o.struct_size,
            o.mode,
            o.generated_frames,
            o.reflex_mode,
            o.frame_limit_us,
            o.debug,
            s.depth_inverted,
            s.depth_infinite,
            s.units_to_meters,
            s.runtime_switching,
            s.ui_mode,
            text(s.fsr3_directory_utf8),
            text(s.fsr4_directory_utf8),
            text(s.xess_directory_utf8),
            text(s.streamline_directory_utf8),
            s.engine_type,
            text(s.engine_version_utf8),
            text(s.project_id_utf8)
        ));
        fake().install_result
    }
    pub(super) fn bridge_open(editor: bool) -> bool {
        assert!(!fake().close_held);
        if editor {
            return true;
        }
        call("surface".into());
        fake().surface
    }
    /// Checks the services the bridge gets and writes the API a bridge writes on success.
    pub(super) unsafe fn bridge_prepare(
        services: *const rsf_game_host_services,
        api: *mut rsf_unity_native_api,
    ) -> u32 {
        let s = unsafe { *services };
        call(format!("prepare {}", s.session_id));
        assert_eq!((s.struct_size, s.abi_version), (104, 24));
        // Its passes and CPU events come to the package first; its plan is the host's own.
        assert_eq!(s.user as usize, HOST_USER);
        assert!(
            s.render_pass.map(|f| f as usize)
                == Some(routing::render_callback as *const () as usize)
        );
        assert!(s.render_config.map(|f| f as usize) == Some(host_config as *const () as usize));
        assert!(s.log.map(|f| f as usize) == Some(services::backend_log as *const () as usize));
        assert!(s.cpu_event.is_some());
        if !fake().prepared {
            return 0;
        }
        unsafe {
            (*api).struct_size = 72;
            (*api).session_id = s.session_id;
        }
        1
    }
    pub(super) fn local_app_data() -> Option<Vec<u16>> {
        call("local".into());
        fake().local.map(|local| local.encode_utf16().collect())
    }
    pub(super) fn create_directory(path: &[u16]) {
        call(format!("mkdir {}", wide(path)));
    }
    pub(super) fn log_open(path: &[u16]) -> *mut c_void {
        call(format!("open {}", wide(path)));
        handle(fake().file)
    }
    pub(super) fn log_write(file: *mut c_void, text: &CStr) {
        let text = text.to_str().unwrap();
        call(if file.is_null() {
            format!("log {text}")
        } else {
            format!("log {:#x} {text}", file as usize)
        });
    }
    pub(super) fn log_close(file: *mut c_void) {
        call(format!("close {:#x}", file as usize));
    }
    pub(super) fn player_window() -> *mut c_void {
        call("player".into());
        handle(fake().player)
    }
    pub(super) fn preview_parent() -> *mut c_void {
        call("parent".into());
        handle(fake().parent)
    }
    pub(super) fn subclass(window: *mut c_void) -> bool {
        call(format!("subclass {:#x}", window as usize));
        fake().subclass_accepted
    }
    pub(super) fn restore(window: *mut c_void) {
        call(format!("restore {:#x}", window as usize));
    }
    pub(super) fn input_kind(message: u32, lparam: isize) -> u32 {
        rsf_windows::classify_window_message(message, || {
            call(format!("raw {lparam}"));
            fake().raw
        })
    }
    pub(super) fn timestamp() -> (u64, u64) {
        (1000, 10)
    }
    pub(super) unsafe fn cpu(event: *const rsf_game_cpu_event) {
        let e = unsafe { *event };
        call(format!(
            "cpu {} {} {} {}/{} {} {:#x} {} {}",
            e.stage,
            e.session_id,
            e.source_frame_id,
            e.timestamp_qpc,
            e.qpc_frequency,
            e.input_kind,
            e.message_id,
            e.flags,
            e.event_version
        ));
    }
    pub(super) unsafe fn preview_present(
        resource: *mut c_void,
        pass: *const rsf_game_render_pass,
        common: bool,
    ) -> bool {
        assert_eq!(resource, unsafe { (*pass).color_input });
        call(format!(
            "present {:#x} {}",
            resource as usize,
            u32::from(common)
        ));
        fake().shown
    }
    pub(super) unsafe fn texture_desc(resource: *mut c_void) -> (u64, u32, u32) {
        call(format!("desc {:#x}", resource as usize));
        (640, 360, 10)
    }
    pub(super) fn preview_prepare(width: u32, height: u32) -> bool {
        call(format!("preview prepare {width}x{height}"));
        fake().preview_ready
    }
    pub(super) fn preview_owns(window: *mut c_void) -> bool {
        call(format!("preview owns {:#x}", window as usize));
        fake().preview_owns
    }
    pub(super) unsafe fn renderer_vulkan(_: *mut c_void) -> u32 {
        call("vulkan renderer".into());
        fake().vulkan.into()
    }
    pub(super) fn vulkan_owns(window: *mut c_void) -> bool {
        call(format!("surface owns {:#x}", window as usize));
        fake().surface_owns
    }
    pub(super) fn window_class(window: *mut c_void) -> u32 {
        call(format!("class {:#x}", window as usize));
        fake().class
    }
    pub(super) unsafe fn bridge_load(_: *mut c_void) -> bool {
        call("bridge load".into());
        fake().registry
    }
    pub(super) fn bridge_unload() {
        call("bridge unload".into());
    }

    /// Fresh fakes, saved settings and an empty call record for this test.
    fn reset() {
        FAKE.with(|cell| cell.set(READY));
        SAVED.with(|saved| saved.set(saved_defaults()));
        calls();
    }

    fn session() -> Identity {
        Identity::new(b"R", b"6000", b"P")
    }

    /// open's calls in the player: the first session opens the process log and hooks the player
    /// window; every session checks the Vulkan surface, attaches the host with that log and
    /// the SDK root's preferences, and prepares the bridge.
    fn opened(first: bool) -> Vec<String> {
        let log = format!("L\\ReScaleFrame\\Unity\\{}.log", std::process::id());
        let mut calls = Vec::new();
        if first {
            calls.extend([
                "local".into(),
                "mkdir L\\ReScaleFrame".into(),
                "mkdir L\\ReScaleFrame\\Unity".into(),
                format!("open {log}"),
                "player".into(),
                "subclass 0x20".into(),
            ]);
        }
        calls.extend([
            "surface".into(),
            format!("host start {log} R\\preferences.ini R\\streamline R\\fidelityfx 2"),
            "prepare 42".into(),
        ]);
        calls
    }

    fn started(package: &Package) -> Result<u64, i32> {
        package.initialize(session()).map(|api| api.session_id)
    }

    #[test]
    fn defaults() {
        let package = Package::new();
        let state = package.lock();
        let s = state.settings;
        assert_eq!((s.struct_size, s.abi_version, s.sr_backend), (100, 2, 7));
        assert_eq!(
            (s.quality, s.dlss_preset, s.fg_backend, s.fg_mode),
            (1, 0, 7, 0)
        );
        assert_eq!(
            (s.generated_frames, s.latency_mode, s.frame_limit_us),
            (1, 0, 0)
        );
        assert_eq!((s.gameplay, s.sharpening.struct_size), (1, 0));
        assert!(!state.pending && state.session == Session::Unopened);
    }

    #[test]
    fn a_request_is_the_hosts_settings_change() {
        let package = Package::new();
        reset();
        let mut input = DEFAULT_SETTINGS;
        input.abi_version = 1;
        assert!(!package.configure(&input));
        // Without a session the request waits for the next session's host.
        input.abi_version = RSF_UNITY_SETTINGS_ABI_VERSION;
        input.quality = 2;
        assert!(package.configure(&input));
        assert_eq!(
            calls(),
            ["apply sr=1/7 q=2 preset=0 fg=0/7 frames=1 latency=0 limit=0 sharpness=0.5"]
        );
        assert!(package.lock().pending);
        // An invalid one is refused without a session as well.
        set(|f| f.apply_result = -1);
        input.quality = 9;
        assert!(!package.configure(&input));
        calls();
        set(|f| f.apply_result = 0);
        input.quality = 2;
        assert_eq!(package.settings().quality, 2);
        // With one the host checks, applies and saves it: no backend is SR off with the saved
        // backend kept, and a zero sharpening size is the Runtime's defaults.
        set(|f| f.attached = true);
        input.sr_backend = RSF_SR_NONE;
        (input.fg_mode, input.generated_frames, input.frame_limit_us) = (1, 3, 8000);
        assert!(package.configure(&input));
        assert_eq!(
            calls(),
            ["apply sr=0/7 q=2 preset=0 fg=1/7 frames=3 latency=0 limit=8000 sharpness=0.5"]
        );
        let read = package.settings();
        assert_eq!(
            (read.sr_backend, read.fg_mode, read.generated_frames),
            (0, 1, 3)
        );
        assert_eq!(read.sharpening.struct_size, 0, "the developer's defaults");
        // A refusal keeps the previous request.
        set(|f| f.apply_result = -1);
        input.generated_frames = 16;
        assert!(!package.configure(&input));
        assert_eq!(package.lock().settings.generated_frames, 3);
        // Sharpening the developer sets is read back from the host.
        set(|f| f.apply_result = 0);
        input.generated_frames = 2;
        input.sharpening = rsf_sharpening_options {
            struct_size: 56,
            abi_version: 1,
            sharpness: 0.25,
            ..Default::default()
        };
        assert!(package.configure(&input));
        calls();
        let read = package.settings();
        assert_eq!(
            (read.sharpening.struct_size, read.sharpening.sharpness),
            (56, 0.25)
        );
    }

    #[test]
    fn status_merges_the_host_and_the_runtime_and_a_bridge_failure() {
        let package = Package::new();
        reset();
        set(|f| f.attached = true);
        let status = package.status();
        assert_eq!(calls(), ["fg status", "backend", "switch", "latency"]);
        assert_eq!(
            (status.requested_sr, status.active_sr, status.sr_result),
            (RSF_SR_AUTO, 3, 0)
        );
        assert_eq!((status.render_width, status.output_height), (1280, 1080));
        assert_eq!((status.evaluated_frames, status.refused_frames), (12, 2));
        assert_eq!(
            (status.fg_backend, status.fg_available, status.fg_active),
            (5, 1, 1)
        );
        assert_eq!(
            (status.max_generated_frames, status.generated_frames),
            (3, 2)
        );
        assert_eq!((status.latency_available, status.latency_mode), (1, 1));
        assert_eq!(
            (status.reason, status.tagged_frames, status.fg_result),
            (3, 9, -7)
        );
        assert_eq!((status.sleep_calls, status.latency_owner), (11, 2));
        assert_eq!(
            (
                status.keyboard_inputs,
                status.mouse_inputs,
                status.controller_inputs
            ),
            (4, 5, 6)
        );
        assert_eq!(status.derived_boundaries, 8);
        assert!(package.temporal_ready());
        // A bridge failure is the SR result and ends temporal readiness.
        set(|f| f.latency = false);
        package.bridge_failed(-9);
        let status = package.status();
        calls();
        assert_eq!((status.sr_result, status.active_sr), (-9, 0));
        assert_eq!((status.latency_available, status.keyboard_inputs), (0, 0));
        assert!(!package.temporal_ready());
        // The engine's scaler and a disabled plan are not temporal; without a host nothing is.
        package.bridge_failed(0);
        set(|f| f.sr.active_backend = RSF_SR_SPATIAL);
        assert!(!package.temporal_ready());
        set(|f| f.attached = false);
        assert!(!package.temporal_ready());
        let status = package.status();
        calls();
        assert_eq!(
            (status.requested_sr, status.evaluated_frames),
            (RSF_SR_AUTO, 0)
        );
        let counters = package.frame_counters();
        assert_eq!((counters.struct_size, counters.abi_version), (40, 1));
        assert_eq!((counters.valid, counters.active), (1, 1));
        assert_eq!(
            (
                counters.total_presented,
                counters.generated_presented,
                counters.provider_generation
            ),
            (40, 20, 77)
        );
    }

    #[test]
    fn sessions_attach_the_host_retire_it_and_refuse_while_busy() {
        let package = Package::new();
        reset();
        package.bridge_failed(-4);
        // A request before the session is applied by its host.
        let mut input = DEFAULT_SETTINGS;
        input.quality = 2;
        assert!(package.configure(&input));
        calls();
        assert_eq!(started(&package), Ok(42));
        assert_eq!(
            calls(),
            [
                opened(true),
                vec![
                    "apply sr=1/7 q=2 preset=0 fg=0/7 frames=1 latency=0 limit=0 sharpness=0.5"
                        .into(),
                    "activate 1".into()
                ]
            ]
            .concat()
        );
        assert!(!package.lock().pending);
        assert_eq!(package.services.session(), 42);
        assert_eq!(package.bridge_result.load(SeqCst), 0);
        assert!(package.lock().session == Session::Started(42));
        assert_eq!(package.presentation.identity().xess.to_bytes(), br"R\xess");
        // A started session only restarts once drained and its own session ended.
        set(|f| f.drained = false);
        assert_eq!(started(&package), Err(code::BUSY));
        assert_eq!(calls(), ["close"]);
        set(|f| f.drained = true);
        set(|f| f.ended = false);
        assert_eq!(started(&package), Err(code::BUSY));
        assert_eq!(calls(), ["close", "end 42", "retain"]);
        assert!(package.host().is_some(), "the host stays with the session");
        set(|f| f.ended = true);
        // A refused bridge stops the host it attached.
        set(|f| f.prepared = false);
        assert_eq!(started(&package), Err(code::FAILED));
        assert_eq!(
            calls(),
            [
                vec![
                    "close".into(),
                    "end 42".into(),
                    "host stop".into(),
                    "release".into()
                ],
                opened(false),
                vec!["host stop".into()]
            ]
            .concat()
        );
        assert!(package.lock().session == Session::Opening);
        assert!(package.host().is_none());
        // A refused host attaches nothing.
        set(|f| f.prepared = true);
        set(|f| f.host_accepts = false);
        assert_eq!(started(&package), Err(code::FAILED));
        let refused = calls();
        assert_eq!(
            refused.last().unwrap(),
            "log 0x10 The Runtime D3D12 host refused to attach; see its log."
        );
        set(|f| f.host_accepts = true);
        assert_eq!(started(&package), Ok(42));
        calls();
        // Shutdown and device shutdown use the same checked transaction as restart.
        set(|f| f.drained = false);
        assert!(!package.retire());
        assert_eq!(calls(), ["close"]);
        set(|f| f.drained = true);
        assert!(package.retire());
        assert_eq!(calls(), ["close", "end 42", "host stop", "release"]);
        assert!(package.lock().session == Session::Stopped);
        set(|f| f.drained = false);
        assert!(package.retire());
        assert!(calls().is_empty());
    }

    #[test]
    fn generation_installs_once_when_identified() {
        let package = Package::new();
        reset();
        package.install();
        assert!(calls().is_empty());
        package.presentation.set_identity(session());
        set(|f| f.install_result = -4);
        package.install();
        // Auto, unlimited, generation off with Reflex off until rsfu_configure, inverted finite
        // depth in metres, switchable, HUD-less backbuffer, Unity's Streamline identity.
        assert_eq!(
            calls(),
            [
                "install 152 7 4294967295 R\\fidelityfx | 24 0 1 0 0 0 | 1 0 1 1 4 | \
                 R\\fidelityfx R\\fidelityfx R\\xess R\\streamline | 2 6000 P"
            ]
        );
        // A refusal is retried; an accepted install is not repeated.
        set(|f| f.install_result = 0);
        package.install();
        assert_eq!(calls().len(), 1);
        package.install();
        assert!(calls().is_empty());
    }

    fn list(address: usize) -> *mut c_void {
        without_provenance_mut(address)
    }
    fn pass(role: u32, view_key: u64) -> rsf_game_render_pass {
        let mut pass = rsf_game_render_pass {
            role,
            view_key,
            camera_valid: 1,
            screen: 7,
            ..Default::default()
        };
        (pass.camera.output_width, pass.camera.output_height) = (8, 6);
        pass
    }

    #[test]
    fn exports_check_structure_sizes() {
        let mut settings = DEFAULT_SETTINGS;
        settings.struct_size -= 1;
        assert_eq!(unsafe { rsfu_configure(&settings) }, 0);
        assert_eq!(unsafe { rsfu_configure(std::ptr::null()) }, 0);
        assert_eq!(unsafe { rsfu_settings(&mut settings) }, 0);
        settings.struct_size += 1;
        settings.abi_version = 1;
        assert_eq!(unsafe { rsfu_settings(&mut settings) }, 0);
        settings.abi_version = 2;
        settings.quality = 4;
        assert_eq!(unsafe { rsfu_settings(&mut settings) }, 1);
        assert_eq!(settings.quality, PACKAGE.lock().settings.quality);
        let mut status = rsf_unity_package_status {
            struct_size: 183,
            ..Default::default()
        };
        assert_eq!(unsafe { rsfu_status(&mut status) }, 0);
        let mut counters = rsf_unity_frame_counters {
            struct_size: 41,
            ..Default::default()
        };
        assert_eq!(unsafe { rsfu_frame_counters(&mut counters) }, 0);
        calls();
        let sr = pass(rsf_game_sys::RSF_GAME_RENDER_SR, 1);
        unsafe {
            routing::render_callback(null_mut(), null_mut(), &sr, 1);
            routing::render_callback(null_mut(), list(1), std::ptr::null(), 1);
        }
        assert!(calls().is_empty());
        let (text, none) = (c"x".as_ptr(), std::ptr::null());
        let mut api: rsf_unity_native_api = Default::default();
        let output = &raw mut api;
        unsafe {
            // Unity's device has not initialized yet.
            assert_eq!(rsfu_initialize(text, text, text, output), code::BUSY);
            assert_eq!(rsfu_initialize(none, text, text, output), code::FAILED);
            assert_eq!(rsfu_initialize(text, none, text, output), code::FAILED);
            assert_eq!(rsfu_initialize(text, text, none, output), code::FAILED);
            assert_eq!(
                rsfu_initialize(text, text, text, std::ptr::null_mut()),
                code::FAILED
            );
        }
        assert!(calls().is_empty());
        assert_eq!(api.struct_size, 0);
    }

    fn wide_text(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn the_player_loads_its_log_and_installs_once() {
        let package = Package::new();
        reset();
        package.presentation.set_identity(session());
        package.loaded(false, &wide_text("C:\\rsf.log"));
        let loaded = calls();
        assert_eq!(loaded[0], "open C:\\rsf.log");
        assert!(loaded[1].starts_with("install 152 7 "));
        assert_eq!(loaded.len(), 2);
        // The configured log stands in for the process log; the host appends to it too.
        package.log("hello");
        assert_eq!(calls(), ["log 0x10 hello"]);
        assert!(package.open());
        assert_eq!(
            calls(),
            [
                "player",
                "subclass 0x20",
                "surface",
                "host start C:\\rsf.log R\\preferences.ini R\\streamline R\\fidelityfx 2",
                "prepare 42"
            ]
        );
        // Unload restores the procedure and closes the log; later lines reach the debugger only.
        package.unload();
        assert_eq!(calls(), ["restore 0x20", "close 0x10"]);
        package.log("late");
        assert_eq!(calls(), ["log late"]);
        package.unload();
        assert!(calls().is_empty());
        // The player has no preview input.
        package.preview_input();
        assert!(calls().is_empty());
    }

    #[test]
    fn the_editor_installs_per_session_and_hooks_the_preview() {
        let package = Package::new();
        reset();
        package.presentation.set_identity(session());
        package.loaded(true, &[]);
        assert!(calls().is_empty());
        // No player window and no Vulkan surface: the preview presents in the Editor.
        assert!(package.open());
        let opened = calls();
        assert_eq!(opened[0], "local");
        assert!(opened[4].starts_with("install 152 7 "));
        assert!(opened[5].starts_with("host start "));
        assert_eq!(opened[6..], ["prepare 42"]);
        // The preview's parent is hooked once it exists, then kept.
        set(|f| f.parent = 0);
        package.preview_input();
        assert_eq!(calls(), ["parent"]);
        set(|f| f.parent = 0x40);
        package.preview_input();
        assert_eq!(calls(), ["parent", "subclass 0x40"]);
        package.preview_input();
        assert!(calls().is_empty());
    }

    #[test]
    fn a_refused_window_subclass_is_retried() {
        let package = Package::new();
        reset();
        package.loaded(true, &[]);
        set(|f| f.subclass_accepted = false);
        package.preview_input();
        assert_eq!(calls(), ["parent", "subclass 0x40"]);
        set(|f| f.subclass_accepted = true);
        package.preview_input();
        assert_eq!(calls(), ["parent", "subclass 0x40"]);
        package.preview_input();
        assert!(calls().is_empty());
    }

    #[test]
    fn a_refused_log_or_surface_is_retried_by_the_next_session() {
        let package = Package::new();
        reset();
        set(|f| f.file = 0);
        package.loaded(false, &wide_text("C:\\rsf.log"));
        assert_eq!(calls(), ["open C:\\rsf.log"]);
        package.presentation.set_identity(session());
        set(|f| f.surface = false);
        assert!(!package.open());
        let mut refused = opened(true);
        refused.truncate(refused.len() - 2);
        refused.push(
            "log Unity graphics bridge or player presentation surface could not open.".into(),
        );
        assert_eq!(calls(), refused);
        // Without %LOCALAPPDATA% there is no process log, and the host's lines are discarded.
        set(|f| f.surface = true);
        set(|f| f.local = None);
        let package = Package::new();
        package.loaded(false, &[]);
        package.presentation.set_identity(session());
        assert!(package.open());
        assert_eq!(
            calls(),
            [
                "local",
                "player",
                "subclass 0x20",
                "surface",
                "host start NUL R\\preferences.ini R\\streamline R\\fidelityfx 2",
                "prepare 42"
            ]
        );
    }

    #[test]
    fn received_input_is_keyed_to_the_cpu_frame_in_progress() {
        use rsf_game_sys::{RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END};
        let package = Package::new();
        reset();
        package.services.session.store(42, SeqCst);
        let frame = |stage, source| rsf_game_cpu_event {
            struct_size: 64,
            stage,
            session_id: 42,
            source_frame_id: source,
            timestamp_qpc: 5,
            qpc_frequency: 10,
            render_expected: 1,
            input_kind: 0,
            message_id: 0,
            event_version: 1,
            flags: 0,
        };
        // Outside a frame a keyed message is unframed; any other message carries no kind.
        package.window_message(0x0100, 0);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 1 0x100 21 1"]);
        package.window_message(0x000f, 0);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 0 0xf 5 1"]);
        // The bridge's events reach the Runtime unchanged.
        package.cpu_event(&frame(RSF_GAME_CPU_FRAME_BEGIN, 7));
        assert_eq!(calls(), ["cpu 0 42 7 5/10 0 0x0 0 1"]);
        package.window_message(0x020e, 0);
        assert_eq!(calls(), ["cpu 5 42 7 1000/10 2 0x20e 5 1"]);
        // WM_INPUT carries its raw device's kind.
        for (raw, kind) in [(Some(1), 1), (Some(0), 2), (Some(2), 0), (None, 0)] {
            set(|f| f.raw = raw);
            package.window_message(0x00ff, 9);
            assert_eq!(
                calls(),
                [
                    "raw 9".into(),
                    format!("cpu 5 42 7 1000/10 {kind} 0xff 5 1")
                ]
            );
        }
        // Managed input is keyed to its frame only while that frame is in progress.
        assert_eq!(package.input(7, 3), backend_result::OK);
        assert_eq!(calls(), ["cpu 5 42 7 1000/10 3 0x0 1 1"]);
        package.input(6, 4);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 4 0x0 17 1"]);
        package.cpu_event(&frame(RSF_GAME_CPU_FRAME_END, 7));
        assert_eq!(calls(), ["cpu 4 42 7 5/10 0 0x0 0 1"]);
        package.input(7, 1);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 1 0x0 17 1"]);
        package.window_message(0x0109, 0);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 1 0x109 21 1"]);
    }

    #[test]
    fn the_player_hands_every_pass_to_the_host_with_its_screen_class() {
        use rsf_game_sys::{
            RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW,
        };
        let package = Package::new();
        reset();
        // Without a host nothing is forwarded.
        package.route(list(1), &pass(RSF_GAME_RENDER_SR, 3), true);
        assert!(calls().is_empty());
        package.presentation.set_identity(session());
        assert!(package.open());
        calls();
        // SR passes carry the developer's gameplay flag, begin and end alike.
        let sr = pass(RSF_GAME_RENDER_SR, 3);
        package.route(list(1), &sr, true);
        package.route(list(1), &sr, false);
        let flight = RSF_SCREEN_FLIGHT;
        assert_eq!(
            calls(),
            [
                format!("host pass 0x1 1 3 1 {flight}"),
                format!("host pass 0x1 1 3 0 {flight}")
            ]
        );
        package.lock().settings.gameplay = 0;
        package.route(list(1), &sr, true);
        assert_eq!(calls(), [format!("host pass 0x1 1 3 1 {RSF_SCREEN_MENU}")]);
        // The window and final scene keep their own.
        package.route(list(1), &pass(RSF_GAME_RENDER_WINDOW, 3), true);
        package.route(list(1), &pass(RSF_GAME_RENDER_FINAL_SCENE, 3), true);
        assert_eq!(calls(), ["host pass 0x1 9 3 1 7", "host pass 0x1 10 3 1 7"]);
    }

    #[test]
    fn the_editor_preview_is_admitted_and_its_refusals_budgeted() {
        use rsf_game_sys::{RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW};
        let package = Package::new();
        reset();
        set(|f| {
            f.shown = false;
            f.preview_ready = false;
        });
        package.presentation.set_identity(session());
        package.loaded(true, &[]);
        assert!(package.open());
        calls();
        let sr = pass(RSF_GAME_RENDER_SR, 3);
        // The SR pass waits for the preview's resources.
        package.route(list(1), &sr, true);
        assert_eq!(calls(), ["preview prepare 8x6"]);
        set(|f| f.preview_ready = true);
        package.route(list(1), &sr, true);
        assert_eq!(
            calls(),
            [
                "preview prepare 8x6".into(),
                format!("host pass 0x1 1 3 1 {RSF_SCREEN_FLIGHT}")
            ]
        );
        // The preview presents the window pass; the host never sees it. A refusal without a
        // texture still spends the budget.
        let mut window = pass(RSF_GAME_RENDER_WINDOW, 3);
        package.route(list(1), &window, true);
        package.route(list(1), &window, false);
        assert_eq!(calls(), ["present 0x0 0"]);
        window.color_input = without_provenance_mut(0x50);
        let logged = "log 0x10 Editor presentation pending: texture=640x360 format=10 expected=8x6";
        for _ in 0..3 {
            package.route(list(1), &window, true);
            assert_eq!(calls(), ["present 0x50 0", "desc 0x50", logged]);
        }
        package.route(list(1), &window, true);
        assert_eq!(calls(), ["present 0x50 0"]);
        // A shown preview neither logs nor counts.
        set(|f| f.shown = true);
        package.route(list(1), &window, true);
        assert_eq!(calls(), ["present 0x50 0"]);
        assert_eq!(package.preview_reports.load(SeqCst), 5);
    }

    #[test]
    fn the_window_guard_follows_the_host() {
        let package = Package::new();
        reset();
        let window = without_provenance_mut(0x60);
        // The D3D12 player accepts its own UnityWndClass window, and another window of this
        // process only when it is the Vulkan surface.
        assert!(package.accept_window(window));
        assert_eq!(calls(), ["vulkan renderer", "class 0x60"]);
        set(|f| {
            f.class = 2;
            f.surface_owns = false;
        });
        assert!(!package.accept_window(window));
        assert_eq!(
            calls(),
            ["vulkan renderer", "class 0x60", "surface owns 0x60"]
        );
        set(|f| f.class = 0);
        assert!(!package.accept_window(window));
        assert_eq!(calls(), ["vulkan renderer", "class 0x60"]);
        // The Vulkan player accepts only its surface.
        set(|f| {
            f.vulkan = true;
            f.surface_owns = true;
        });
        assert!(package.accept_window(window));
        assert_eq!(calls(), ["vulkan renderer", "surface owns 0x60"]);
        // The Editor accepts only its preview.
        package.services.editor.store(true, SeqCst);
        set(|f| f.preview_owns = false);
        assert!(!package.accept_window(window));
        assert_eq!(calls(), ["preview owns 0x60"]);
    }

    #[test]
    fn the_device_lifecycle_loads_retires_and_retains() {
        let package = Package::new();
        reset();
        // Waiting: nothing to retire, and the bridge loads once Unity's interfaces exist.
        assert!(package.shutdown());
        package.device_event(false);
        assert!(calls().is_empty());
        set(|f| f.registry = false);
        package.device_event(true);
        assert_eq!(calls(), ["bridge load"]);
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_WAITING);
        set(|f| f.registry = true);
        package.device_event(true);
        package.device_event(true);
        assert_eq!(calls(), ["bridge load"]);
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_READY);
        // Ready: a shutdown retires the session, then unloads the bridge.
        package.device_event(false);
        assert_eq!(calls(), ["close", "release", "bridge unload"]);
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_WAITING);
        // A refused retirement retains the device for the process.
        package.device_event(true);
        package.lock().session = Session::Opening;
        set(|f| f.drained = false);
        calls();
        package.device_event(false);
        assert_eq!(
            calls(),
            [
                "close",
                "log Unity device shutdown retained unproved bridge work; initialization requires a process restart."
            ]
        );
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_RETAINED);
        assert!(!package.shutdown());
        package.device_event(true);
        package.device_event(false);
        assert!(calls().is_empty());
    }
}
