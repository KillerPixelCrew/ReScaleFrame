// SPDX-License-Identifier: MIT
//! The Unity package's state behind ReScaleFrame.Unity.Native.dll's rsfu_* exports
//! (integrations/unity/native/package.cpp): the stored settings and their validation, the status
//! snapshot, the render plan, temporal readiness, the pending reconfiguration, the session
//! lifecycle with its result codes, and the sequencing of the render callback's D3D12 SR views
//! (their table and slot rules are package_views.rs). package.cpp keeps the Unity, DXGI, Vulkan
//! and preview work, the render callback's routing and the input window, and lends the SDK and
//! Runtime calls through rsf_unity_package_platform (integrations/unity/native/package_state.h);
//! nothing here loads a Runtime or references its exports.
//!
//! package.cpp's mutex still serializes the exports and the render path where it did. The state
//! lock here only makes the state sound for the callers that never took that mutex (Unity's
//! device shutdown, renderer bridge failures, temporal readiness), and it is never held across a
//! leaf. The view lock is held across the SR leaves, which never re-enter, so Unity's device
//! shutdown cannot destroy a context mid-evaluation; it is always taken before the state lock.
use crate::package_abi::{
    RSF_FG_STAT_TOTAL_PRESENTED, RSF_SR_NONE, RSF_UNITY_SETTINGS_ABI_VERSION,
    rsf_native_fg_options, rsf_native_fg_status, rsf_native_latency_status, rsf_sharpening_options,
    rsf_sr12, rsf_unity_frame_counters, rsf_unity_package_platform, rsf_unity_package_settings,
    rsf_unity_package_status, rsf_unity_package_view_setup,
};
use crate::package_views::{View, Views};
use crate::runtime_abi::{
    RSF_FG_BACKEND_AUTO, RSF_FG_BACKEND_DLSS, RSF_FG_BACKEND_FSR3, RSF_FG_BACKEND_FSR4,
    RSF_FG_BACKEND_XESS, RSF_FG_FIXED, RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_SPATIAL,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_RECONFIGURE, rsf_camera_frame, rsf_game_render_config, rsf_game_render_pass,
};
use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// RSF_BACKEND_OK, a cast macro bindgen does not evaluate; package.cpp asserts it.
const BACKEND_OK: i32 = 0;
/// RSF_SCREEN_MENU and RSF_SCREEN_FLIGHT, cast macros; package.cpp asserts them. The stored
/// gameplay flag picks the screen class of every SR pass.
const SCREEN_MENU: u32 = 1;
const SCREEN_FLIGHT: u32 = 4;
/// rsfu_initialize's results, part of the managed contract (ReScaleFrameRuntime.Apply).
mod code {
    pub(super) const FAILED: i32 = 0;
    pub(super) const STARTED: i32 = 1;
    /// The previous session still owns GPU work or Runtime tokens.
    pub(super) const BUSY: i32 = 2;
}
/// The status and frame-counter versions the C++ wrote; the latency status request version.
const STATUS_ABI_VERSION: u32 = 1;
const FRAME_COUNTERS_ABI_VERSION: u32 = 1;
const LATENCY_STATUS_VERSION: u32 = 1;
/// ReconstructionQuality.UltraQuality, the highest quality and one DLSS does not offer.
const ULTRA_QUALITY: u32 = 5;
const MAX_LATENCY_MODE: u32 = 2;
const MAX_GENERATED_FRAMES: u32 = 15;
const FG_BACKENDS: [u32; 5] = [
    RSF_FG_BACKEND_DLSS,
    RSF_FG_BACKEND_FSR3,
    RSF_FG_BACKEND_FSR4,
    RSF_FG_BACKEND_XESS,
    RSF_FG_BACKEND_AUTO,
];

/// The settings before the first rsfu_configure. A zero sharpening struct_size selects
/// rsf_sharpening_defaults at evaluation.
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
const EMPTY_PLAN: rsf_game_render_config = rsf_game_render_config {
    struct_size: size_of::<rsf_game_render_config>() as u32,
    enabled: 0,
    output_width: 0,
    output_height: 0,
    render_width: 0,
    render_height: 0,
};

const fn fresh_status() -> rsf_unity_package_status {
    // SAFETY: every field is an integer, for which zero is valid.
    let mut status: rsf_unity_package_status = unsafe { std::mem::zeroed() };
    status.struct_size = size_of::<rsf_unity_package_status>() as u32;
    status.abi_version = STATUS_ABI_VERSION;
    status
}

/// package.cpp's leaves, all present.
struct Leaves {
    sharpening_valid: unsafe extern "C" fn(*const rsf_sharpening_options) -> u32,
    dlss_preset_valid: unsafe extern "C" fn(u32) -> u32,
    set_engine_spatial: unsafe extern "C" fn(u32, u32) -> c_int,
    fg_options: unsafe extern "C" fn(*const rsf_native_fg_options),
    fg_status: unsafe extern "C" fn(*mut rsf_native_fg_status) -> c_int,
    latency_status: unsafe extern "C" fn(*mut rsf_native_latency_status) -> c_int,
    present_backend: unsafe extern "C" fn() -> u32,
    present_switch_result: unsafe extern "C" fn() -> i32,
    present_generation: unsafe extern "C" fn() -> u64,
    open: unsafe extern "C" fn(*mut c_void) -> u32,
    bridge_activate: unsafe extern "C" fn(u32),
    bridge_drained: unsafe extern "C" fn() -> u32,
    end_session: unsafe extern "C" fn() -> u32,
    sr_default_backend: unsafe extern "C" fn(*mut c_void, *mut u32) -> u32,
    sr_create: unsafe extern "C" fn(
        *mut c_void,
        *const rsf_unity_package_view_setup,
        *mut i32,
        *mut *mut rsf_sr12,
    ) -> u32,
    sr_plan: unsafe extern "C" fn(*const rsf_sr12, *mut u32, *mut u32) -> i32,
    sharpening_defaults: unsafe extern "C" fn(*mut rsf_sharpening_options),
    sr_set_sharpening: unsafe extern "C" fn(*mut rsf_sr12, *const rsf_sharpening_options) -> i32,
    sr_evaluate:
        unsafe extern "C" fn(*mut rsf_sr12, *mut c_void, *const rsf_game_render_pass, u32) -> i32,
    sr_destroy: unsafe extern "C" fn(*mut rsf_sr12),
    log: unsafe extern "C" fn(*const c_char),
    bridge_release: unsafe extern "C" fn(),
}

// SAFETY for every call below: the leaves are package.cpp's noexcept functions and borrow their
// pointer arguments for the call only.
impl Leaves {
    /// The table is package.cpp's constant; an incomplete one is a build defect.
    unsafe fn adopt(platform: *const rsf_unity_package_platform) -> Option<Self> {
        if platform.is_null() {
            return None;
        }
        let p = unsafe { *platform };
        Some(Self {
            sharpening_valid: p.sharpening_valid?,
            dlss_preset_valid: p.dlss_preset_valid?,
            set_engine_spatial: p.set_engine_spatial?,
            fg_options: p.fg_options?,
            fg_status: p.fg_status?,
            latency_status: p.latency_status?,
            present_backend: p.present_backend?,
            present_switch_result: p.present_switch_result?,
            present_generation: p.present_generation?,
            open: p.open?,
            bridge_activate: p.bridge_activate?,
            bridge_drained: p.bridge_drained?,
            end_session: p.end_session?,
            sr_default_backend: p.sr_default_backend?,
            sr_create: p.sr_create?,
            sr_plan: p.sr_plan?,
            sharpening_defaults: p.sharpening_defaults?,
            sr_set_sharpening: p.sr_set_sharpening?,
            sr_evaluate: p.sr_evaluate?,
            sr_destroy: p.sr_destroy?,
            log: p.log?,
            bridge_release: p.bridge_release?,
        })
    }

    fn sharpening_valid(&self, options: &rsf_sharpening_options) -> bool {
        unsafe { (self.sharpening_valid)(options) != 0 }
    }
    fn dlss_preset_valid(&self, preset: u32) -> bool {
        unsafe { (self.dlss_preset_valid)(preset) != 0 }
    }
    /// The bridge always accepts the policy; its result was never read.
    fn set_engine_spatial(&self, spatial: bool, quality: u32) {
        unsafe { (self.set_engine_spatial)(u32::from(spatial), quality) };
    }
    fn fg_options(&self, options: &rsf_native_fg_options) {
        unsafe { (self.fg_options)(options) }
    }
    /// The Runtime's generation status; zero-filled apart from struct_size when it has none, as
    /// the C++ never read the result.
    fn fg_status(&self) -> rsf_native_fg_status {
        let mut status = rsf_native_fg_status {
            struct_size: size_of::<rsf_native_fg_status>() as u32,
            ..Default::default()
        };
        unsafe { (self.fg_status)(&mut status) };
        status
    }
    fn latency_status(&self) -> Option<rsf_native_latency_status> {
        let mut status = rsf_native_latency_status {
            struct_size: size_of::<rsf_native_latency_status>() as u32,
            version: LATENCY_STATUS_VERSION,
            ..Default::default()
        };
        (unsafe { (self.latency_status)(&mut status) } != 0).then_some(status)
    }
    fn present_backend(&self) -> u32 {
        unsafe { (self.present_backend)() }
    }
    fn present_switch_result(&self) -> i32 {
        unsafe { (self.present_switch_result)() }
    }
    fn present_generation(&self) -> u64 {
        unsafe { (self.present_generation)() }
    }
    fn open(&self, user: *mut c_void) -> bool {
        unsafe { (self.open)(user) != 0 }
    }
    fn bridge_activate(&self, enabled: bool) {
        unsafe { (self.bridge_activate)(u32::from(enabled)) }
    }
    /// Drained, then the Runtime's session retired, in that order and only if drained.
    fn drained_and_ended(&self) -> bool {
        unsafe { (self.bridge_drained)() != 0 && (self.end_session)() != 0 }
    }
    fn bridge_drained(&self) -> bool {
        unsafe { (self.bridge_drained)() != 0 }
    }
    /// Auto's backend for the list's adapter; None when its device or adapter cannot be read.
    fn sr_default_backend(&self, commands: *mut c_void) -> Option<u32> {
        let mut backend = 0u32;
        (unsafe { (self.sr_default_backend)(commands, &mut backend) } != 0).then_some(backend)
    }
    /// rsf_sr12_create's result and context (null unless created); None when the list has no
    /// device.
    fn sr_create(
        &self,
        commands: *mut c_void,
        setup: &rsf_unity_package_view_setup,
    ) -> Option<(i32, *mut rsf_sr12)> {
        let (mut result, mut sr): (i32, *mut rsf_sr12) = (0, null_mut());
        (unsafe { (self.sr_create)(commands, setup, &mut result, &mut sr) } != 0)
            .then_some((result, sr))
    }
    /// The context's planned input extents. The C++ never read the result; they stay zero on a
    /// refusal.
    fn sr_plan(&self, sr: *mut rsf_sr12) -> (u32, u32) {
        let (mut width, mut height) = (0u32, 0u32);
        unsafe { (self.sr_plan)(sr, &mut width, &mut height) };
        (width, height)
    }
    /// The stored sharpening, or the Runtime defaults while its struct_size is zero.
    fn sharpening(&self, stored: &rsf_sharpening_options) -> rsf_sharpening_options {
        if stored.struct_size != 0 {
            return *stored;
        }
        let mut defaults = rsf_sharpening_options::default();
        unsafe { (self.sharpening_defaults)(&mut defaults) };
        defaults
    }
    /// The C++ never read the result.
    fn sr_set_sharpening(&self, sr: *mut rsf_sr12, options: &rsf_sharpening_options) {
        unsafe { (self.sr_set_sharpening)(sr, options) };
    }
    fn sr_evaluate(
        &self,
        sr: *mut rsf_sr12,
        commands: *mut c_void,
        pass: &rsf_game_render_pass,
        slot: u32,
    ) -> i32 {
        unsafe { (self.sr_evaluate)(sr, commands, pass, slot) }
    }
    fn sr_destroy(&self, sr: *mut rsf_sr12) {
        unsafe { (self.sr_destroy)(sr) }
    }
    fn log(&self, text: &str) {
        if let Ok(text) = CString::new(text) {
            unsafe { (self.log)(text.as_ptr()) }
        }
    }
    fn bridge_release(&self) {
        unsafe { (self.bridge_release)() }
    }
}

struct State {
    settings: rsf_unity_package_settings,
    status: rsf_unity_package_status,
    plan: rsf_game_render_config,
    /// A settings change waits for a reconfiguring pass; no evaluation until then.
    pending: bool,
    started: bool,
}

struct Package {
    state: Mutex<State>,
    views: Mutex<Views>,
    temporal_ready: AtomicBool,
    /// A renderer bridge failure, reported as the SR result until the next session.
    bridge_result: AtomicI32,
}

static PACKAGE: Package = Package::new();

/// Everything the C++ range-checked, in its order: the sharpening and preset leaves are only
/// called once the fields before them passed.
fn acceptable(input: &rsf_unity_package_settings, leaves: &Leaves) -> bool {
    input.abi_version == RSF_UNITY_SETTINGS_ABI_VERSION
        && input.sr_backend <= RSF_SR_AUTO
        && (input.sharpening.struct_size == 0 || leaves.sharpening_valid(&input.sharpening))
        && input.quality <= ULTRA_QUALITY
        && leaves.dlss_preset_valid(input.dlss_preset)
        && input.fg_mode <= RSF_FG_FIXED
        && input.latency_mode <= MAX_LATENCY_MODE
        && (1..=MAX_GENERATED_FRAMES).contains(&input.generated_frames)
        && FG_BACKENDS.contains(&input.fg_backend)
}

impl Package {
    const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                settings: DEFAULT_SETTINGS,
                status: fresh_status(),
                plan: EMPTY_PLAN,
                pending: false,
                started: false,
            }),
            views: Mutex::new(Views::new()),
            temporal_ready: AtomicBool::new(false),
            bridge_result: AtomicI32::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_views(&self) -> MutexGuard<'_, Views> {
        self.views.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores valid settings. A reconstruction change (backend, quality or DLSS model) sets the
    /// engine policy, withdraws the plan and temporal readiness and waits for the bridge's
    /// reconfiguring pass. DLSS has no ultra quality, including Auto while DLSS is active.
    fn configure(&self, input: &rsf_unity_package_settings, leaves: &Leaves) -> bool {
        if !acceptable(input, leaves) {
            return false;
        }
        let reconstruction_changed = {
            let state = self.lock();
            if input.quality == ULTRA_QUALITY
                && (input.sr_backend == RSF_SR_DLSS
                    || (input.sr_backend == RSF_SR_AUTO && state.status.active_sr == RSF_SR_DLSS))
            {
                return false;
            }
            state.settings.sr_backend != input.sr_backend
                || state.settings.quality != input.quality
                || state.settings.dlss_preset != input.dlss_preset
        };
        if reconstruction_changed {
            leaves.set_engine_spatial(input.sr_backend == RSF_SR_SPATIAL, input.quality);
        }
        {
            let mut state = self.lock();
            if reconstruction_changed {
                state.pending = true;
                state.plan.enabled = 0;
                self.temporal_ready.store(false, SeqCst);
            }
            state.settings = *input;
            state.status.requested_sr = input.sr_backend;
        }
        leaves.fg_options(&rsf_native_fg_options {
            struct_size: size_of::<rsf_native_fg_options>() as u32,
            mode: input.fg_mode,
            generated_frames: input.generated_frames,
            reflex_mode: input.latency_mode,
            frame_limit_us: input.frame_limit_us,
            debug: 0,
        });
        true
    }

    /// The stored status merged with the Runtime's generation and latency status. The merge is
    /// kept, as the C++ kept it. Leaves are called in the C++ order: generation status, present
    /// backend, the switch result only without a generation result, latency status.
    fn status(&self, leaves: &Leaves) -> rsf_unity_package_status {
        let fg = leaves.fg_status();
        let fg_backend = leaves.present_backend();
        let fg_result = if fg.last_result != 0 {
            fg.last_result
        } else {
            leaves.present_switch_result()
        };
        let latency = leaves.latency_status();
        let mut state = self.lock();
        let status = &mut state.status;
        status.fg_backend = fg_backend;
        status.fg_available = fg.available;
        status.fg_active = fg.vendor.active;
        status.max_generated_frames = fg.vendor.max_generated_frames;
        status.generated_frames = fg.vendor.effective_generated_frames;
        status.latency_available = fg.vendor.low_latency_available;
        status.latency_mode = fg.vendor.effective_reflex;
        status.reason = fg.reason;
        status.tagged_frames = fg.tagged_frames;
        status.fg_result = fg_result;
        let failure = self.bridge_result.load(SeqCst);
        if failure != 0 {
            status.sr_result = failure;
            status.active_sr = 0;
        }
        if let Some(latency) = latency {
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
        }
        *status
    }

    fn frame_counters(&self, leaves: &Leaves) -> rsf_unity_frame_counters {
        let fg = leaves.fg_status();
        rsf_unity_frame_counters {
            struct_size: size_of::<rsf_unity_frame_counters>() as u32,
            abi_version: FRAME_COUNTERS_ABI_VERSION,
            valid: u32::from(fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0),
            active: fg.vendor.active,
            total_presented: fg.vendor.total_presented,
            generated_presented: fg.vendor.generated_presented,
            provider_generation: leaves.present_generation(),
        }
    }

    fn initialize(&self, leaves: &Leaves, user: *mut c_void) -> i32 {
        let started = self.lock().started;
        if started {
            if !leaves.drained_and_ended() {
                return code::BUSY;
            }
            self.retire(leaves);
        }
        self.bridge_result.store(0, SeqCst);
        {
            let mut state = self.lock();
            state.plan = EMPTY_PLAN;
            state.status = fresh_status();
            state.status.requested_sr = state.settings.sr_backend;
        }
        if !leaves.open(user) {
            return code::FAILED;
        }
        let settings = self.lock().settings;
        leaves.set_engine_spatial(settings.sr_backend == RSF_SR_SPATIAL, settings.quality);
        leaves.bridge_activate(true);
        self.lock().started = true;
        code::STARTED
    }

    fn quiesce(&self, leaves: &Leaves) -> bool {
        leaves.bridge_activate(false);
        leaves.drained_and_ended()
    }

    fn retire(&self, leaves: &Leaves) {
        self.views_released();
        destroy_views(&mut self.lock_views(), leaves);
        leaves.bridge_release();
        self.lock().started = false;
    }

    fn device_shutdown(&self, leaves: &Leaves) {
        leaves.bridge_activate(false);
        if leaves.bridge_drained() {
            self.retire(leaves);
        }
    }

    /// No view, so no temporal reconstruction is active or ready.
    fn views_released(&self) {
        self.temporal_ready.store(false, SeqCst);
        self.lock().status.active_sr = 0;
    }

    fn reconfigured(&self) {
        self.views_released();
        self.lock().pending = false;
    }

    fn view_planned(&self, backend: u32, output: (u32, u32), render: (u32, u32)) {
        let mut state = self.lock();
        state.plan = rsf_game_render_config {
            struct_size: size_of::<rsf_game_render_config>() as u32,
            enabled: 1,
            output_width: output.0,
            output_height: output.1,
            render_width: render.0,
            render_height: render.1,
        };
        self.temporal_ready.store(backend != 0, SeqCst);
        let status = &mut state.status;
        (status.render_width, status.render_height) = render;
        (status.output_width, status.output_height) = output;
    }

    /// The SR result follows the evaluation while a temporal backend, nothing or the engine's
    /// spatial path was requested, and always on a refusal. A view without a temporal backend
    /// that stands in for one keeps its creation result.
    fn evaluated(&self, result: i32, view_backend: u32) {
        let mut state = self.lock();
        let requested = state.settings.sr_backend;
        let status = &mut state.status;
        if view_backend != 0 || requested == RSF_SR_NONE || requested == RSF_SR_SPATIAL {
            status.sr_result = result;
        }
        if result == BACKEND_OK {
            status.evaluated_frames = status.evaluated_frames.wrapping_add(1);
            status.active_sr = if requested == RSF_SR_SPATIAL {
                RSF_SR_SPATIAL
            } else {
                view_backend
            };
        } else {
            status.refused_frames = status.refused_frames.wrapping_add(1);
            status.sr_result = result;
        }
    }

    fn bridge_failed(&self, result: i32) {
        self.bridge_result.store(result, SeqCst);
        self.temporal_ready.store(false, SeqCst);
    }

    /// The SR pass of package.cpp's render callback, in the C++ order: settings are read once,
    /// a reconfiguring pass releases every view, a view whose extents no longer fit is
    /// destroyed, a missing one is created, and evaluation waits for a valid camera, the end of
    /// a pending reconfiguration and a recording slot.
    fn render(&self, leaves: &Leaves, commands: *mut c_void, original: &rsf_game_render_pass) {
        let mut views = self.lock_views();
        let settings = self.lock().settings;
        let mut pass = *original;
        pass.screen = if settings.gameplay != 0 {
            SCREEN_FLIGHT
        } else {
            SCREEN_MENU
        };
        if pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0 {
            self.reconfigured();
            destroy_views(&mut views, leaves);
        }
        let camera = &pass.camera;
        let view = views.entry(pass.view_key);
        if view.stale(
            (camera.output_width, camera.output_height),
            (camera.render_width, camera.render_height),
        ) {
            leaves.sr_destroy(view.sr);
            *view = View::EMPTY;
        }
        if view.sr.is_null() {
            match self.create_view(leaves, commands, &settings, camera) {
                Some(created) => *view = created,
                None => return,
            }
        }
        if pass.camera_valid == 0 || self.lock().pending {
            return;
        }
        let Some(slot) = view.slot(commands) else {
            return;
        };
        leaves.sr_set_sharpening(view.sr, &leaves.sharpening(&settings.sharpening));
        let result = leaves.sr_evaluate(view.sr, commands, &pass, slot);
        self.evaluated(result, view.backend);
    }

    /// A new view's context: Auto resolved for the list's adapter, the engine's spatial path as
    /// backend 0, and a vendor backend that created nothing retried once as backend 0. The first
    /// creation's result is the SR result. None leaves the view empty for the next pass.
    fn create_view(
        &self,
        leaves: &Leaves,
        commands: *mut c_void,
        settings: &rsf_unity_package_settings,
        camera: &rsf_camera_frame,
    ) -> Option<View> {
        let mut backend = match settings.sr_backend {
            RSF_SR_AUTO => leaves.sr_default_backend(commands)?,
            requested => requested,
        };
        if backend == RSF_SR_SPATIAL {
            backend = 0;
        }
        let output = (camera.output_width, camera.output_height);
        let mut setup = rsf_unity_package_view_setup {
            backend,
            quality: settings.quality,
            dlss_preset: settings.dlss_preset,
            output_width: output.0,
            output_height: output.1,
            render_width: camera.render_width,
            render_height: camera.render_height,
            inverted_depth: camera.depth_inverted,
        };
        let (created, mut sr) = leaves.sr_create(commands, &setup)?;
        self.lock().status.sr_result = created;
        if sr.is_null() && backend != 0 {
            setup.backend = 0;
            sr = leaves
                .sr_create(commands, &setup)
                .map_or(null_mut(), |(_, sr)| sr);
            backend = 0;
        }
        if sr.is_null() {
            return None;
        }
        let mut view = View::created(sr, backend, output);
        view.render = leaves.sr_plan(sr);
        self.view_planned(backend, output, view.render);
        leaves.log(&format!(
            "URP SR requested={} active={backend} result={created} {}x{} -> {}x{}",
            settings.sr_backend, view.render.0, view.render.1, output.0, output.1
        ));
        Some(view)
    }
}

/// Destroys every view's context. Their submissions are retired: a reconfiguring pass follows
/// the bridge's wait for its command ring, and session retirement follows the bridge's drain.
fn destroy_views(views: &mut Views, leaves: &Leaves) {
    for sr in views.take() {
        leaves.sr_destroy(sr);
    }
}

/// Only the size field is readable from a short structure.
unsafe fn struct_size<T>(value: *const T) -> u32 {
    unsafe { value.cast::<u32>().read() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_configure(
    input: *const rsf_unity_package_settings,
    platform: *const rsf_unity_package_platform,
) -> c_int {
    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return 0;
    };
    if input.is_null()
        || unsafe { struct_size(input) } != size_of::<rsf_unity_package_settings>() as u32
    {
        return 0;
    }
    c_int::from(PACKAGE.configure(&unsafe { input.read() }, &leaves))
}

/// Accepts a longer structure, as the C++ did, and writes only this version's fields.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_settings_get(
    output: *mut rsf_unity_package_settings,
) -> c_int {
    if output.is_null()
        || unsafe { struct_size(output) } < size_of::<rsf_unity_package_settings>() as u32
        || unsafe { (*output).abi_version } != RSF_UNITY_SETTINGS_ABI_VERSION
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.lock().settings) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_status_get(
    output: *mut rsf_unity_package_status,
    platform: *const rsf_unity_package_platform,
) -> c_int {
    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return 0;
    };
    if output.is_null()
        || unsafe { struct_size(output) } != size_of::<rsf_unity_package_status>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.status(&leaves)) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_frame_counters(
    output: *mut rsf_unity_frame_counters,
    platform: *const rsf_unity_package_platform,
) -> c_int {
    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return 0;
    };
    if output.is_null()
        || unsafe { struct_size(output) } != size_of::<rsf_unity_frame_counters>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.frame_counters(&leaves)) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_temporal_ready() -> u32 {
    u32::from(PACKAGE.temporal_ready.load(SeqCst))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_initialize(
    platform: *const rsf_unity_package_platform,
    user: *mut c_void,
) -> c_int {
    match unsafe { Leaves::adopt(platform) } {
        Some(leaves) => PACKAGE.initialize(&leaves, user),
        None => code::FAILED,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_quiesce(platform: *const rsf_unity_package_platform) -> u32 {
    u32::from(unsafe { Leaves::adopt(platform) }.is_some_and(|leaves| PACKAGE.quiesce(&leaves)))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_retire(platform: *const rsf_unity_package_platform) {
    if let Some(leaves) = unsafe { Leaves::adopt(platform) } {
        PACKAGE.retire(&leaves);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_device_shutdown(
    platform: *const rsf_unity_package_platform,
) {
    if let Some(leaves) = unsafe { Leaves::adopt(platform) } {
        PACKAGE.device_shutdown(&leaves);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_current(output: *mut rsf_unity_package_settings) {
    if !output.is_null() {
        unsafe { output.write(PACKAGE.lock().settings) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_plan(output: *mut rsf_game_render_config) -> c_int {
    if output.is_null()
        || unsafe { struct_size(output) } < size_of::<rsf_game_render_config>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.lock().plan) };
    1
}

/// package.cpp checked both pointers before its editor preview, as the C++ render path did.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_render(
    commands: *mut c_void,
    pass: *const rsf_game_render_pass,
    platform: *const rsf_unity_package_platform,
) {
    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return;
    };
    if commands.is_null() || pass.is_null() {
        return;
    }
    PACKAGE.render(&leaves, commands, unsafe { &*pass });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_fg_result(result: i32) {
    PACKAGE.lock().status.fg_result = result;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_bridge_failed(result: i32) {
    PACKAGE.bridge_failed(result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_abi::rsf_fg_status;
    use std::cell::{Cell, RefCell};
    use std::ffi::CStr;
    use std::ptr::without_provenance_mut;

    #[derive(Clone, Copy)]
    struct Fake {
        sharpening_valid: bool,
        preset_valid: bool,
        drained: bool,
        ended: bool,
        opened: bool,
        last_result: i32,
        switch_result: i32,
        latency: bool,
        /// Auto's backend for the adapter; zero when it cannot be read.
        adapter: u32,
        /// The command list has a device.
        device: bool,
        /// A vendor backend creates a context; backend 0 always does.
        vendor_creates: bool,
        create_result: i32,
        evaluate_result: i32,
        /// A context's planned input extents.
        plan: (u32, u32),
    }

    const READY: Fake = Fake {
        sharpening_valid: true,
        preset_valid: true,
        drained: true,
        ended: true,
        opened: true,
        last_result: 0,
        switch_result: -7,
        latency: true,
        adapter: 3,
        device: true,
        vendor_creates: true,
        create_result: 0,
        evaluate_result: 0,
        plan: (1280, 720),
    };
    /// A fake context's address is 0x100 plus its backend.
    const CONTEXT: usize = 0x100;

    thread_local! {
        static FAKE: Cell<Fake> = const { Cell::new(READY) };
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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

    unsafe extern "C" fn sharpening_valid(_: *const rsf_sharpening_options) -> u32 {
        call("sharpening".into());
        fake().sharpening_valid.into()
    }
    unsafe extern "C" fn dlss_preset_valid(_: u32) -> u32 {
        call("preset".into());
        fake().preset_valid.into()
    }
    unsafe extern "C" fn set_engine_spatial(spatial: u32, quality: u32) -> c_int {
        call(format!("spatial {spatial} {quality}"));
        1
    }
    unsafe extern "C" fn fg_options(options: *const rsf_native_fg_options) {
        let o = unsafe { *options };
        call(format!(
            "options {} {} {} {} {} {}",
            o.struct_size, o.mode, o.generated_frames, o.reflex_mode, o.frame_limit_us, o.debug
        ));
    }
    unsafe extern "C" fn fg_status(status: *mut rsf_native_fg_status) -> c_int {
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
    unsafe extern "C" fn latency_status(status: *mut rsf_native_latency_status) -> c_int {
        call("latency".into());
        let status = unsafe { &mut *status };
        assert_eq!((status.struct_size, status.version), (152, 1));
        if !fake().latency {
            return 0;
        }
        status.paced_frames = 11;
        status.owner = 2;
        status.events.input_events = [4, 5, 6];
        status.derived_boundaries = 8;
        1
    }
    unsafe extern "C" fn present_backend() -> u32 {
        call("backend".into());
        5
    }
    unsafe extern "C" fn present_switch_result() -> i32 {
        call("switch".into());
        fake().switch_result
    }
    unsafe extern "C" fn present_generation() -> u64 {
        call("generation".into());
        77
    }
    unsafe extern "C" fn open(user: *mut c_void) -> u32 {
        call(format!("open {}", user as usize));
        fake().opened.into()
    }
    unsafe extern "C" fn bridge_activate(enabled: u32) {
        call(format!("activate {enabled}"));
    }
    unsafe extern "C" fn bridge_drained() -> u32 {
        call("drained".into());
        fake().drained.into()
    }
    unsafe extern "C" fn end_session() -> u32 {
        call("end".into());
        fake().ended.into()
    }
    unsafe extern "C" fn sr_default_backend(_: *mut c_void, backend: *mut u32) -> u32 {
        call("default".into());
        let adapter = fake().adapter;
        if adapter == 0 {
            return 0;
        }
        unsafe { backend.write(adapter) };
        1
    }
    unsafe extern "C" fn sr_create(
        _: *mut c_void,
        setup: *const rsf_unity_package_view_setup,
        result: *mut i32,
        output: *mut *mut rsf_sr12,
    ) -> u32 {
        let s = unsafe { *setup };
        call(format!(
            "create {} {} {} {}x{} {}x{} {}",
            s.backend,
            s.quality,
            s.dlss_preset,
            s.output_width,
            s.output_height,
            s.render_width,
            s.render_height,
            s.inverted_depth
        ));
        let f = fake();
        if !f.device {
            return 0;
        }
        let created = s.backend == 0 || f.vendor_creates;
        unsafe {
            result.write(f.create_result);
            output.write(if created {
                without_provenance_mut(CONTEXT + s.backend as usize)
            } else {
                null_mut()
            });
        }
        1
    }
    unsafe extern "C" fn sr_plan(_: *const rsf_sr12, width: *mut u32, height: *mut u32) -> i32 {
        call("plan".into());
        let (planned_width, planned_height) = fake().plan;
        unsafe {
            width.write(planned_width);
            height.write(planned_height);
        }
        0
    }
    unsafe extern "C" fn sharpening_defaults(output: *mut rsf_sharpening_options) {
        call("defaults".into());
        unsafe { (*output).struct_size = 56 };
    }
    unsafe extern "C" fn sr_set_sharpening(
        _: *mut rsf_sr12,
        options: *const rsf_sharpening_options,
    ) -> i32 {
        call(format!("sharpen {}", unsafe { (*options).struct_size }));
        -1
    }
    unsafe extern "C" fn sr_evaluate(
        sr: *mut rsf_sr12,
        _: *mut c_void,
        pass: *const rsf_game_render_pass,
        slot: u32,
    ) -> i32 {
        let screen = unsafe { (*pass).screen };
        call(format!("evaluate {:#x} {slot} {screen}", sr as usize));
        fake().evaluate_result
    }
    unsafe extern "C" fn sr_destroy(sr: *mut rsf_sr12) {
        call(format!("destroy {:#x}", sr as usize));
    }
    unsafe extern "C" fn log(text: *const c_char) {
        call(format!(
            "log {}",
            unsafe { CStr::from_ptr(text) }.to_str().unwrap()
        ));
    }
    unsafe extern "C" fn bridge_release() {
        call("release".into());
    }

    fn platform() -> rsf_unity_package_platform {
        rsf_unity_package_platform {
            sharpening_valid: Some(sharpening_valid),
            dlss_preset_valid: Some(dlss_preset_valid),
            set_engine_spatial: Some(set_engine_spatial),
            fg_options: Some(fg_options),
            fg_status: Some(fg_status),
            latency_status: Some(latency_status),
            present_backend: Some(present_backend),
            present_switch_result: Some(present_switch_result),
            present_generation: Some(present_generation),
            open: Some(open),
            bridge_activate: Some(bridge_activate),
            bridge_drained: Some(bridge_drained),
            end_session: Some(end_session),
            sr_default_backend: Some(sr_default_backend),
            sr_create: Some(sr_create),
            sr_plan: Some(sr_plan),
            sharpening_defaults: Some(sharpening_defaults),
            sr_set_sharpening: Some(sr_set_sharpening),
            sr_evaluate: Some(sr_evaluate),
            sr_destroy: Some(sr_destroy),
            log: Some(log),
            bridge_release: Some(bridge_release),
        }
    }
    /// Fresh fakes and an empty call record for this test.
    fn leaves() -> Leaves {
        FAKE.with(|cell| cell.set(READY));
        calls();
        unsafe { Leaves::adopt(&platform()) }.unwrap()
    }

    #[test]
    fn an_incomplete_table_is_refused() {
        let mut table = platform();
        table.bridge_release = None;
        assert!(unsafe { Leaves::adopt(&table) }.is_none());
        assert!(unsafe { Leaves::adopt(std::ptr::null()) }.is_none());
    }

    #[test]
    fn defaults_match_the_cpp() {
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
        assert_eq!(
            (state.status.struct_size, state.status.abi_version),
            (184, 1)
        );
        assert_eq!((state.plan.struct_size, state.plan.enabled), (24, 0));
        assert!(!state.pending && !state.started);
    }

    #[test]
    fn configuration_is_range_checked_in_order() {
        let package = Package::new();
        let leaves = leaves();
        let valid = DEFAULT_SETTINGS;
        let refused: [fn(&mut rsf_unity_package_settings); 10] = [
            |s| s.abi_version = 1,
            |s| s.sr_backend = 8,
            |s| s.quality = 6,
            |s| s.fg_mode = 2,
            |s| s.latency_mode = 3,
            |s| s.generated_frames = 0,
            |s| s.generated_frames = 16,
            |s| s.fg_backend = 2,
            |s| s.fg_backend = 6,
            |s| s.fg_backend = 0,
        ];
        for change in refused {
            let mut input = valid;
            change(&mut input);
            assert!(!package.configure(&input, &leaves));
        }
        calls();
        // A zero sharpening size skips its check; any other size is validated.
        let mut input = valid;
        input.sharpening.struct_size = 56;
        set(|f| f.sharpening_valid = false);
        assert!(!package.configure(&input, &leaves));
        assert_eq!(calls(), ["sharpening"]);
        set(|f| f.sharpening_valid = true);
        set(|f| f.preset_valid = false);
        assert!(!package.configure(&input, &leaves));
        assert_eq!(calls(), ["sharpening", "preset"]);
        set(|f| f.preset_valid = true);
        // Nothing was stored by the refusals.
        assert_eq!(package.lock().settings.quality, 1);
        for backend in FG_BACKENDS {
            let mut input = valid;
            input.fg_backend = backend;
            assert!(package.configure(&input, &leaves));
        }
    }

    #[test]
    fn dlss_has_no_ultra_quality() {
        let package = Package::new();
        let leaves = leaves();
        let mut input = DEFAULT_SETTINGS;
        input.quality = ULTRA_QUALITY;
        input.sr_backend = RSF_SR_DLSS;
        assert!(!package.configure(&input, &leaves));
        input.sr_backend = RSF_SR_AUTO;
        package.lock().status.active_sr = RSF_SR_DLSS;
        assert!(!package.configure(&input, &leaves));
        package.lock().status.active_sr = 5;
        assert!(package.configure(&input, &leaves));
    }

    #[test]
    fn a_reconstruction_change_waits_for_reconfiguration() {
        let package = Package::new();
        let leaves = leaves();
        package.view_planned(3, (1920, 1080), (1280, 720));
        assert!(package.temporal_ready.load(SeqCst));
        calls();
        // Generation settings alone leave the plan and readiness in place.
        let mut input = DEFAULT_SETTINGS;
        input.fg_mode = 1;
        input.generated_frames = 3;
        input.latency_mode = 2;
        input.frame_limit_us = 16_666;
        assert!(package.configure(&input, &leaves));
        assert_eq!(calls(), ["preset", "options 24 1 3 2 16666 0"]);
        assert!(package.temporal_ready.load(SeqCst));
        assert!(!package.lock().pending);
        assert_eq!(package.lock().status.requested_sr, RSF_SR_AUTO);
        // The engine's spatial path is a reconstruction change.
        input.sr_backend = RSF_SR_SPATIAL;
        input.quality = 3;
        assert!(package.configure(&input, &leaves));
        assert_eq!(
            calls(),
            ["preset", "spatial 1 3", "options 24 1 3 2 16666 0"]
        );
        assert!(!package.temporal_ready.load(SeqCst));
        {
            let state = package.lock();
            assert!(state.pending);
            assert_eq!(state.plan.enabled, 0);
            assert_eq!(state.plan.render_width, 1280);
            assert_eq!(state.status.requested_sr, RSF_SR_SPATIAL);
            assert_eq!(state.settings.quality, 3);
        }
        input.dlss_preset = 10;
        input.sr_backend = RSF_SR_DLSS;
        assert!(package.configure(&input, &leaves));
        assert_eq!(
            calls(),
            ["preset", "spatial 0 3", "options 24 1 3 2 16666 0"]
        );
        package.lock().status.active_sr = 3;
        package.reconfigured();
        assert!(!package.lock().pending);
        assert_eq!(package.lock().status.active_sr, 0);
    }

    #[test]
    fn evaluation_results_follow_the_requested_path() {
        let package = Package::new();
        package.lock().status.sr_result = 4;
        // Auto fell back to a view without a temporal backend: the creation result stays.
        package.evaluated(BACKEND_OK, 0);
        let status = package.lock().status;
        assert_eq!(
            (status.sr_result, status.active_sr, status.evaluated_frames),
            (4, 0, 1)
        );
        package.evaluated(BACKEND_OK, 3);
        assert_eq!(package.lock().status.active_sr, 3);
        assert_eq!(package.lock().status.sr_result, BACKEND_OK);
        package.evaluated(-2, 0);
        let status = package.lock().status;
        assert_eq!(
            (status.sr_result, status.refused_frames, status.active_sr),
            (-2, 1, 3)
        );
        package.lock().settings.sr_backend = RSF_SR_SPATIAL;
        package.evaluated(BACKEND_OK, 0);
        let status = package.lock().status;
        assert_eq!(
            (status.sr_result, status.active_sr),
            (BACKEND_OK, RSF_SR_SPATIAL)
        );
        package.lock().settings.sr_backend = RSF_SR_NONE;
        package.lock().status.sr_result = 4;
        package.evaluated(BACKEND_OK, 0);
        assert_eq!(package.lock().status.sr_result, BACKEND_OK);
        assert_eq!(package.lock().status.evaluated_frames, 4);
    }

    #[test]
    fn status_merges_the_runtime_and_a_bridge_failure() {
        let package = Package::new();
        let leaves = leaves();
        package.lock().status.active_sr = 3;
        calls();
        let status = package.status(&leaves);
        assert_eq!(calls(), ["fg status", "backend", "switch", "latency"]);
        assert_eq!(
            (status.fg_backend, status.fg_available, status.fg_active),
            (5, 1, 1)
        );
        assert_eq!(
            (status.max_generated_frames, status.generated_frames),
            (3, 2)
        );
        assert_eq!((status.latency_available, status.latency_mode), (1, 2));
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
        assert_eq!((status.derived_boundaries, status.active_sr), (8, 3));
        // A generation result wins over the switch result, which is then not read.
        set(|f| f.last_result = -3);
        set(|f| f.latency = false);
        package.bridge_failed(-9);
        package.lock().status.sleep_calls = 1;
        let status = package.status(&leaves);
        assert_eq!(calls(), ["fg status", "backend", "latency"]);
        assert_eq!(
            (status.fg_result, status.sr_result, status.active_sr),
            (-3, -9, 0)
        );
        assert_eq!(status.sleep_calls, 1);
        // The merge is stored.
        assert_eq!(package.lock().status.fg_backend, 5);
        let counters = package.frame_counters(&leaves);
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
    fn sessions_start_retire_and_refuse_while_busy() {
        let package = Package::new();
        let leaves = leaves();
        package.lock().settings.quality = 2;
        package.bridge_failed(-4);
        let user = std::ptr::without_provenance_mut::<c_void>(0x40);
        assert_eq!(package.initialize(&leaves, user), code::STARTED);
        assert_eq!(calls(), ["open 64", "spatial 0 2", "activate 1"]);
        assert_eq!(package.bridge_result.load(SeqCst), 0);
        assert!(package.lock().started);
        // A started session only restarts once drained and ended.
        package.view_planned(3, (8, 8), (4, 4));
        package.lock_views().entry(1).sr = without_provenance_mut(0x300);
        package.lock().status.evaluated_frames = 5;
        set(|f| f.drained = false);
        assert_eq!(package.initialize(&leaves, user), code::BUSY);
        assert_eq!(calls(), ["drained"]);
        set(|f| f.drained = true);
        set(|f| f.ended = false);
        assert_eq!(package.initialize(&leaves, user), code::BUSY);
        assert_eq!(calls(), ["drained", "end"]);
        set(|f| f.ended = true);
        set(|f| f.opened = false);
        assert_eq!(package.initialize(&leaves, user), code::FAILED);
        assert_eq!(
            calls(),
            ["drained", "end", "destroy 0x300", "release", "open 64"]
        );
        {
            let state = package.lock();
            assert!(!state.started);
            assert_eq!((state.status.evaluated_frames, state.plan.enabled), (0, 0));
        }
        assert!(!package.temporal_ready.load(SeqCst));
        set(|f| f.opened = true);
        package.lock().settings.sr_backend = RSF_SR_SPATIAL;
        assert_eq!(package.initialize(&leaves, user), code::STARTED);
        assert_eq!(calls(), ["open 64", "spatial 1 2", "activate 1"]);
        assert_eq!(package.lock().status.requested_sr, RSF_SR_SPATIAL);
        // rsfu_shutdown's unlocked half, then its locked half.
        set(|f| f.drained = false);
        assert!(!package.quiesce(&leaves));
        assert_eq!(calls(), ["activate 0", "drained"]);
        set(|f| f.drained = true);
        assert!(package.quiesce(&leaves));
        assert_eq!(calls(), ["activate 0", "drained", "end"]);
        package.retire(&leaves);
        assert_eq!(calls(), ["release"]);
        assert!(!package.lock().started);
        // Unity's device shutdown never ends the Runtime session itself.
        package.lock_views().entry(2);
        package.device_shutdown(&leaves);
        assert_eq!(calls(), ["activate 0", "drained", "destroy 0x0", "release"]);
        set(|f| f.drained = false);
        package.device_shutdown(&leaves);
        assert_eq!(calls(), ["activate 0", "drained"]);
    }

    fn list(address: usize) -> *mut c_void {
        without_provenance_mut(address)
    }
    fn sr_pass(view_key: u64, output: (u32, u32), render: (u32, u32)) -> rsf_game_render_pass {
        // SAFETY: integers, floats and nullable pointers, for which zero is valid.
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.view_key = view_key;
        pass.camera_valid = 1;
        pass.screen = 7;
        (pass.camera.output_width, pass.camera.output_height) = output;
        (pass.camera.render_width, pass.camera.render_height) = render;
        pass.camera.depth_inverted = 1;
        pass
    }

    #[test]
    fn a_view_is_created_once_and_evaluated_in_its_bound_slot() {
        let package = Package::new();
        let leaves = leaves();
        let pass = sr_pass(5, (1920, 1080), (960, 540));
        // Auto resolves for the adapter; the stored gameplay flag selects the flight screen.
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(
            calls(),
            [
                "default",
                "create 3 1 0 1920x1080 960x540 1",
                "plan",
                "log URP SR requested=7 active=3 result=0 1280x720 -> 1920x1080",
                "defaults",
                "sharpen 56",
                "evaluate 0x103 0 4",
            ]
        );
        assert!(package.temporal_ready.load(SeqCst));
        {
            let state = package.lock();
            assert_eq!((state.plan.enabled, state.plan.render_width), (1, 1280));
            let status = state.status;
            assert_eq!(
                (status.sr_result, status.active_sr, status.evaluated_frames),
                (0, 3, 1)
            );
            assert_eq!((status.render_width, status.output_height), (1280, 1080));
        }
        // The view keeps its context and its list's slot; another list binds the next slot.
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(calls(), ["defaults", "sharpen 56", "evaluate 0x103 0 4"]);
        package.render(&leaves, list(0xb0), &pass);
        assert_eq!(calls(), ["defaults", "sharpen 56", "evaluate 0x103 1 4"]);
        // A temporal context keeps other engine render extents; stored sharpening is applied.
        package.lock().settings.sharpening.struct_size = 99;
        package.lock().settings.gameplay = 0;
        package.render(&leaves, list(0xb0), &sr_pass(5, (1920, 1080), (1000, 500)));
        assert_eq!(calls(), ["sharpen 99", "evaluate 0x103 1 1"]);
        // A refusal is counted and becomes the SR result.
        set(|f| f.evaluate_result = -6);
        package.render(&leaves, list(0xb0), &pass);
        calls();
        let status = package.lock().status;
        assert_eq!((status.refused_frames, status.sr_result), (1, -6));
        // Other output extents replace the context, whose new slots start unbound.
        set(|f| f.evaluate_result = 0);
        package.lock().settings.sr_backend = RSF_SR_DLSS;
        let wide = sr_pass(5, (2560, 1440), (960, 540));
        package.render(&leaves, list(0xc0), &wide);
        assert_eq!(
            calls(),
            [
                "destroy 0x103",
                "create 1 1 0 2560x1440 960x540 1",
                "plan",
                "log URP SR requested=1 active=1 result=0 1280x720 -> 2560x1440",
                "sharpen 99",
                "evaluate 0x101 0 1",
            ]
        );
        // A fourth list finds every slot of the view bound and is not evaluated.
        package.render(&leaves, list(0xd0), &wide);
        package.render(&leaves, list(0xe0), &wide);
        assert_eq!(calls().last().unwrap(), "evaluate 0x101 2 1");
        package.render(&leaves, list(0xf0), &wide);
        assert!(calls().is_empty());
    }

    #[test]
    fn a_vendor_failure_falls_back_to_backend_zero() {
        let package = Package::new();
        let leaves = leaves();
        package.lock().settings.sr_backend = RSF_SR_DLSS;
        set(|f| f.vendor_creates = false);
        set(|f| f.create_result = -5);
        // Backend 0 plans the engine's render extents.
        set(|f| f.plan = (960, 540));
        let pass = sr_pass(1, (1920, 1080), (960, 540));
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(
            calls(),
            [
                "create 1 1 0 1920x1080 960x540 1",
                "create 0 1 0 1920x1080 960x540 1",
                "plan",
                "log URP SR requested=1 active=0 result=-5 960x540 -> 1920x1080",
                "defaults",
                "sharpen 56",
                "evaluate 0x100 0 4",
            ]
        );
        // The stand-in keeps the creation result; nothing temporal is ready or active.
        assert!(!package.temporal_ready.load(SeqCst));
        let status = package.lock().status;
        assert_eq!((status.sr_result, status.active_sr), (-5, 0));
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(calls(), ["defaults", "sharpen 56", "evaluate 0x100 0 4"]);
        // A backend-zero context is replaced for other engine render extents.
        set(|f| f.plan = (1000, 500));
        package.render(&leaves, list(0xa0), &sr_pass(1, (1920, 1080), (1000, 500)));
        assert_eq!(
            calls(),
            [
                "destroy 0x100",
                "create 1 1 0 1920x1080 1000x500 1",
                "create 0 1 0 1920x1080 1000x500 1",
                "plan",
                "log URP SR requested=1 active=0 result=-5 1000x500 -> 1920x1080",
                "defaults",
                "sharpen 56",
                "evaluate 0x100 0 4",
            ]
        );
        // The engine's spatial path asks for backend 0 directly and reports itself active.
        package.lock().settings.sr_backend = RSF_SR_SPATIAL;
        set(|f| f.create_result = 0);
        package.render(&leaves, list(0xa0), &sr_pass(2, (1920, 1080), (960, 540)));
        assert_eq!(calls()[0], "create 0 1 0 1920x1080 960x540 1");
        assert_eq!(package.lock().status.active_sr, RSF_SR_SPATIAL);
    }

    #[test]
    fn creation_and_evaluation_wait_for_their_inputs() {
        let package = Package::new();
        let leaves = leaves();
        package.lock().status.sr_result = 4;
        let pass = sr_pass(1, (1920, 1080), (960, 540));
        // An unreadable adapter or a list without a device leaves the view empty, unreported.
        set(|f| f.adapter = 0);
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(calls(), ["default"]);
        package.lock().settings.sr_backend = RSF_SR_DLSS;
        set(|f| f.device = false);
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(calls(), ["create 1 1 0 1920x1080 960x540 1"]);
        assert_eq!(package.lock().status.sr_result, 4);
        assert!(package.lock_views().entry(1).sr.is_null());
        // An invalid camera or a pending reconfiguration creates without evaluating.
        set(|f| f.device = true);
        let mut probe = pass;
        probe.camera_valid = 0;
        package.render(&leaves, list(0xa0), &probe);
        assert_eq!(
            calls().last().unwrap(),
            "log URP SR requested=1 active=1 result=0 1280x720 -> 1920x1080"
        );
        package.lock().pending = true;
        package.render(&leaves, list(0xa0), &pass);
        assert!(calls().is_empty());
        // A reconfiguring pass releases every view, then plans the new one without evaluating.
        package.lock_views().entry(9);
        package.lock().status.active_sr = 1;
        probe.flags = RSF_GAME_RENDER_RECONFIGURE;
        package.render(&leaves, list(0xa0), &probe);
        assert_eq!(
            calls(),
            [
                "destroy 0x101",
                "destroy 0x0",
                "create 1 1 0 1920x1080 960x540 1",
                "plan",
                "log URP SR requested=1 active=1 result=0 1280x720 -> 1920x1080",
            ]
        );
        assert!(!package.lock().pending);
        assert!(package.temporal_ready.load(SeqCst));
        package.render(&leaves, list(0xa0), &pass);
        assert_eq!(calls().last().unwrap(), "evaluate 0x101 0 4");
    }

    #[test]
    fn exports_check_structure_sizes() {
        let table = platform();
        let mut settings = DEFAULT_SETTINGS;
        settings.struct_size -= 1;
        assert_eq!(unsafe { rsf_unity_package_configure(&settings, &table) }, 0);
        assert_eq!(
            unsafe { rsf_unity_package_configure(std::ptr::null(), &table) },
            0
        );
        assert_eq!(unsafe { rsf_unity_package_settings_get(&mut settings) }, 0);
        settings.struct_size += 1;
        settings.abi_version = 1;
        assert_eq!(unsafe { rsf_unity_package_settings_get(&mut settings) }, 0);
        settings.abi_version = 2;
        settings.quality = 4;
        assert_eq!(unsafe { rsf_unity_package_settings_get(&mut settings) }, 1);
        assert_eq!(settings.quality, PACKAGE.lock().settings.quality);
        let mut status = rsf_unity_package_status {
            struct_size: 183,
            ..Default::default()
        };
        assert_eq!(
            unsafe { rsf_unity_package_status_get(&mut status, &table) },
            0
        );
        let mut counters = rsf_unity_frame_counters {
            struct_size: 41,
            ..Default::default()
        };
        assert_eq!(
            unsafe { rsf_unity_package_frame_counters(&mut counters, &table) },
            0
        );
        let mut plan = rsf_game_render_config {
            struct_size: 23,
            ..EMPTY_PLAN
        };
        assert_eq!(unsafe { rsf_unity_package_plan(&mut plan) }, 0);
        plan.struct_size = 24;
        assert_eq!(unsafe { rsf_unity_package_plan(&mut plan) }, 1);
        calls();
        let pass = sr_pass(1, (8, 8), (4, 4));
        unsafe {
            rsf_unity_package_render(std::ptr::null_mut(), &pass, &table);
            rsf_unity_package_render(list(1), std::ptr::null(), &table);
            rsf_unity_package_render(list(1), &pass, std::ptr::null());
        }
        assert!(calls().is_empty());
        assert_eq!(
            unsafe { rsf_unity_package_initialize(std::ptr::null(), std::ptr::null_mut()) },
            code::FAILED
        );
    }
}
