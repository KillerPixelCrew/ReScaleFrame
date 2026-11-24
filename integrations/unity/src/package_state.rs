// SPDX-License-Identifier: MIT
//! The Unity package's state behind ReScaleFrame.Unity.Native.dll's rsfu_* exports
//! (integrations/unity/native/package.c): the stored settings and their validation, the status
//! snapshot, the render plan, temporal readiness, the pending reconfiguration, the session
//! lifecycle with its result codes, the sequencing of the render callback's D3D12 SR views
//! (their table and slot rules are package_views.rs), the presentation: the package identity,
//! the generation install and the window pass's provider request (presentation.rs), and the
//! session services: each session's host services and bridge API, the log, the input window and
//! the routing of CPU events, window messages and rsfu_input (services.rs), and the render
//! callback's routing, the generation window guard and Unity's device lifecycle (routing.rs).
//! Rust serializes the session exports, Unity device events and SR evaluation with OPERATIONS.
//! The inner state lock is released across leaf calls. The view and presentation locks
//! precede the state lock. The log and input-window locks cover their own leaves only.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
mod native;
mod presentation;
mod routing;
pub(crate) mod services;
#[cfg(windows)]
mod windows;

use self::presentation::{Identity, Presentation};
use self::services::Services;
use crate::package_views::{Recording, SLOTS, View, Views};
use crate::sys::rsf_unity_native_api;
use crate::sys::{
    RSF_FG_BACKEND_AUTO, RSF_FG_BACKEND_DLSS, RSF_FG_BACKEND_FSR3, RSF_FG_BACKEND_FSR4,
    RSF_FG_BACKEND_XESS, RSF_FG_FIXED, RSF_SR_AUTO, RSF_SR_SPATIAL,
};
use crate::sys::{
    RSF_FG_STAT_TOTAL_PRESENTED, RSF_SR_NONE, RSF_UNITY_SETTINGS_ABI_VERSION,
    rsf_native_fg_options, rsf_native_fg_status, rsf_native_latency_status, rsf_sharpening_options,
    rsf_sr12, rsf_unity_frame_counters, rsf_unity_package_settings, rsf_unity_package_status,
    rsf_unity_package_view_setup,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_RECONFIGURE, rsf_camera_frame, rsf_game_render_config, rsf_game_render_pass,
};
use rsf_game_sys::{RSF_SCREEN_FLIGHT, RSF_SCREEN_MENU, backend_result};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU8, AtomicU32, AtomicU64, Ordering::SeqCst,
};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// RSF_SCREEN_MENU and RSF_SCREEN_FLIGHT, cast macros; package.c asserts them. The stored
/// gameplay flag picks the screen class of every SR pass.
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

static OPERATIONS: Mutex<()> = Mutex::new(());
static UNITY_INTERFACES: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static UNITY_GRAPHICS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// Plugin load bound the leaves; every entry before it refuses.
static LOADED: AtomicBool = AtomicBool::new(false);

fn loaded() -> Option<&'static Leaves> {
    LOADED.load(SeqCst).then_some(&Leaves)
}

/// Package callbacks and native leaves, called through `raw`: native.rs's bindings in production,
/// the test module's fakes under test. Plugin load makes them available (`loaded`).
struct Leaves;

#[cfg(not(test))]
use native::raw;
#[cfg(test)]
use tests as raw;

// SAFETY for every call below: the leaves borrow their pointer arguments for the call only.
impl Leaves {
    fn sharpening_valid(&self, options: &rsf_sharpening_options) -> bool {
        unsafe { raw::sharpening_valid(options) != 0 }
    }
    fn dlss_preset_valid(&self, preset: u32) -> bool {
        unsafe { raw::dlss_preset_valid(preset) != 0 }
    }
    /// The bridge always accepts the policy; its result was never read.
    fn set_engine_spatial(&self, spatial: bool, quality: u32) {
        raw::set_engine_spatial(u32::from(spatial), quality);
    }
    fn fg_options(&self, options: &rsf_native_fg_options) {
        unsafe { raw::fg_options(options) }
    }
    /// The Runtime's generation status; zero-filled apart from struct_size when it has none.
    fn fg_status(&self) -> rsf_native_fg_status {
        let mut status = rsf_native_fg_status {
            struct_size: size_of::<rsf_native_fg_status>() as u32,
            ..Default::default()
        };
        unsafe { raw::fg_status(&mut status) };
        status
    }
    fn latency_status(&self) -> Option<rsf_native_latency_status> {
        let mut status = rsf_native_latency_status {
            struct_size: size_of::<rsf_native_latency_status>() as u32,
            version: LATENCY_STATUS_VERSION,
            ..Default::default()
        };
        (unsafe { raw::latency_status(&mut status) } != 0).then_some(status)
    }
    fn present_backend(&self) -> u32 {
        unsafe { raw::present_backend() }
    }
    fn present_switch_result(&self) -> i32 {
        unsafe { raw::present_switch_result() }
    }
    fn present_generation(&self) -> u64 {
        unsafe { raw::present_generation() }
    }
    fn bridge_activate(&self, enabled: bool) {
        raw::bridge_activate(u32::from(enabled))
    }
    fn bridge_try_close(&self) -> bool {
        raw::bridge_try_close() != 0
    }
    fn end_session(&self, session: u64) -> bool {
        unsafe { raw::end_session(session) != 0 }
    }
    fn sr_quality_supported(&self, backend: u32, quality: u32) -> bool {
        unsafe { raw::sr_quality_supported(backend, quality) != 0 }
    }

    fn sr_create(
        &self,
        commands: *mut c_void,
        setup: &mut rsf_unity_package_view_setup,
    ) -> Option<(i32, *mut rsf_sr12)> {
        let (mut result, mut sr): (i32, *mut rsf_sr12) = (0, null_mut());
        (unsafe { raw::sr_create(commands, setup, &mut result, &mut sr) } != 0)
            .then_some((result, sr))
    }

    /// The stored sharpening, or the Runtime defaults while its struct_size is zero.
    fn sharpening(&self, stored: &rsf_sharpening_options) -> rsf_sharpening_options {
        if stored.struct_size != 0 {
            return *stored;
        }
        let mut defaults = rsf_sharpening_options::default();
        unsafe { raw::sharpening_defaults(&mut defaults) };
        defaults
    }
    /// The result is not read.
    fn sr_set_sharpening(&self, sr: *mut rsf_sr12, options: &rsf_sharpening_options) {
        unsafe { raw::sr_set_sharpening(sr, options) };
    }
    fn sr_evaluate(
        &self,
        sr: *mut rsf_sr12,
        commands: *mut c_void,
        pass: &rsf_game_render_pass,
        slot: u32,
    ) -> i32 {
        unsafe { raw::sr_evaluate(sr, commands, pass, slot) }
    }
    fn sr_destroy(&self, sr: *mut rsf_sr12) {
        unsafe { raw::sr_destroy(sr) }
    }
    fn bridge_finish_retirement(&self, commit: bool) {
        raw::bridge_finish_retirement(u32::from(commit))
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

struct State {
    settings: rsf_unity_package_settings,
    status: rsf_unity_package_status,
    plan: rsf_game_render_config,
    /// A settings change waits for a reconfiguring pass; no evaluation until then.
    pending: bool,
    session: Session,
}

struct Package {
    state: Mutex<State>,
    views: Mutex<Views>,
    presentation: Presentation,
    services: Services,
    temporal_ready: AtomicBool,
    /// A renderer bridge failure, reported as the SR result until the next session.
    bridge_result: AtomicI32,
    /// Unity's device (routing.rs), written only under the package operation lock.
    device: AtomicU8,
    /// Editor preview refusals counted toward the process's diagnostic budget.
    preview_reports: AtomicU32,
}

static PACKAGE: Package = Package::new();
static NEXT_RING_EPOCH: AtomicU64 = AtomicU64::new(1);
// These bridge owners prove fence retirement or discard before reporting each reset.
static D3D11_RECORDINGS: Mutex<ProducerRing> = Mutex::new(ProducerRing::new());
static D3D12_RECORDINGS: Mutex<ProducerRing> = Mutex::new(ProducerRing::new());

struct ProducerRing {
    epoch: u64,
    serials: [u64; SLOTS],
}

impl ProducerRing {
    const fn new() -> Self {
        Self {
            epoch: 0,
            serials: [0; SLOTS],
        }
    }

    fn reset(&mut self, commands: *mut c_void, slot: u32) {
        let Some(serial) = self.serials.get_mut(slot as usize) else {
            RECORDING.set(None);
            return;
        };
        if self.epoch == 0 {
            self.epoch = rsf_unity_package_sr_ring_begin();
        }
        let retired = *serial;
        let Some(next) = retired.checked_add(1) else {
            RECORDING.set(None);
            return;
        };
        *serial = next;
        rsf_unity_package_sr_recording(commands, self.epoch, slot, next, retired);
    }

    fn released(&mut self) {
        rsf_unity_package_sr_ring_retired(self.epoch);
        *self = Self::new();
    }
}

fn d3d_ring(bridge: u32) -> Option<MutexGuard<'static, ProducerRing>> {
    let owner = match bridge {
        11 => &D3D11_RECORDINGS,
        12 => &D3D12_RECORDINGS,
        _ => return None,
    };
    Some(owner.lock().unwrap_or_else(PoisonError::into_inner))
}

pub(crate) extern "C" fn rsf_unity_package_sr_slot_reset(
    bridge: u32,
    commands: *mut c_void,
    slot: u32,
) {
    if let Some(mut ring) = d3d_ring(bridge) {
        ring.reset(commands, slot);
    }
}

pub(crate) extern "C" fn rsf_unity_package_sr_slots_released(bridge: u32) {
    if let Some(mut ring) = d3d_ring(bridge) {
        ring.released();
    }
}

thread_local! {
    // A bridge publishes the reset on the render thread before it forwards the pass.
    static RECORDING: Cell<Option<(*mut c_void, Recording)>> = const { Cell::new(None) };
}

pub(crate) fn rsf_unity_package_sr_ring_begin() -> u64 {
    NEXT_RING_EPOCH
        .fetch_update(SeqCst, SeqCst, |epoch| epoch.checked_add(1))
        .unwrap_or(0)
}

pub(super) extern "C" fn rsf_unity_package_sr_recording(
    commands: *mut c_void,
    epoch: u64,
    slot: u32,
    serial: u64,
    retired_through: u64,
) {
    RECORDING.set(None);
    if commands.is_null()
        || epoch == 0
        || slot as usize >= SLOTS
        || serial == 0
        || retired_through >= serial
    {
        return;
    }
    if retired_through != 0 {
        PACKAGE
            .lock_views()
            .retire_slot(epoch, slot, retired_through);
    }
    RECORDING.set(Some((
        commands,
        Recording {
            epoch,
            slot,
            serial,
        },
    )));
}

pub(crate) fn rsf_unity_package_sr_ring_retired(epoch: u64) {
    if epoch != 0 {
        PACKAGE.lock_views().retire_ring(epoch);
        RECORDING.with(|record| {
            if record
                .get()
                .is_some_and(|(_, recording)| recording.epoch == epoch)
            {
                record.set(None);
            }
        });
    }
}

/// The settings range checks, in order: the sharpening and preset leaves are only called once
/// the fields before them passed.
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
                session: Session::Unopened,
            }),
            views: Mutex::new(Views::new()),
            presentation: Presentation::new(),
            services: Services::new(),
            temporal_ready: AtomicBool::new(false),
            bridge_result: AtomicI32::new(0),
            device: AtomicU8::new(routing::DEVICE_WAITING),
            preview_reports: AtomicU32::new(0),
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
    /// reconfiguring pass. Auto quality admission happens on the current graphics device.
    fn configure(&self, input: &rsf_unity_package_settings, leaves: &Leaves) -> bool {
        if !acceptable(input, leaves)
            || !leaves.sr_quality_supported(input.sr_backend, input.quality)
        {
            return false;
        }
        let reconstruction_changed = {
            let state = self.lock();
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

    /// The stored status merged with the Runtime's generation and latency status. Leaves are called
    /// in order: generation status, present
    /// backend, provider-switch result, latency status.
    fn status(&self, leaves: &Leaves) -> rsf_unity_package_status {
        let fg = leaves.fg_status();
        let fg_backend = leaves.present_backend();
        let fg_result = rsf_game_sys::presentation_result::PresentationResult {
            session: fg.last_result,
            provider_switch: leaves.present_switch_result(),
        }
        .aggregate();
        let latency = leaves.latency_status().unwrap_or_default();
        let mut state = self.lock();
        let status = &mut state.status;
        status.fg_backend = fg_backend;
        status.fg_available = fg.available;
        status.fg_active = fg.vendor.active;
        status.max_generated_frames = fg.vendor.max_generated_frames;
        status.generated_frames = fg.vendor.effective_generated_frames;
        status.reason = fg.reason;
        status.tagged_frames = fg.tagged_frames;
        status.fg_result = fg_result;
        let failure = self.bridge_result.load(SeqCst);
        if failure != 0 {
            status.sr_result = failure;
            status.active_sr = 0;
        }
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

    /// The session's identity replaces the loaded one before open, which may install from it.
    /// The bridge's managed API is returned once the session has started.
    fn initialize(&self, leaves: &Leaves, identity: Identity) -> Result<rsf_unity_native_api, i32> {
        let session = self.lock().session;
        if matches!(session, Session::Opening | Session::Started(_)) && !self.retire(leaves) {
            return Err(code::BUSY);
        }
        self.bridge_result.store(0, SeqCst);
        {
            let mut state = self.lock();
            state.plan = EMPTY_PLAN;
            state.status = fresh_status();
            state.status.requested_sr = state.settings.sr_backend;
            state.session = Session::Opening;
        }
        self.presentation.set_identity(identity);
        if !self.open(leaves) {
            return Err(code::FAILED);
        }
        let settings = self.lock().settings;
        leaves.set_engine_spatial(settings.sr_backend == RSF_SR_SPATIAL, settings.quality);
        self.lock().session = Session::Started(self.services.session());
        leaves.bridge_activate(true);
        Ok(self.services.api())
    }

    fn retire(&self, leaves: &Leaves) -> bool {
        let session = self.lock().session;
        if session == Session::Stopped {
            return true;
        }
        if !leaves.bridge_try_close() {
            return false;
        }
        // Native admission and the Vulkan presentation lease hold the checked fence proof.
        if let Session::Started(id) = session
            && !leaves.end_session(id)
        {
            leaves.bridge_finish_retirement(false);
            return false;
        }
        self.views_released();
        destroy_views(&mut self.lock_views(), leaves);
        leaves.bridge_finish_retirement(true);
        self.services.retired();
        self.lock().session = Session::Stopped;
        true
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
        if result == backend_result::OK {
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

    /// The SR pass of the render callback: settings are read once,
    /// a reconfiguring pass releases every view, a view whose extents no longer fit is
    /// destroyed, a missing one is created, and evaluation waits for a valid camera, the end of
    /// a pending reconfiguration and a recording slot.
    fn render(&self, leaves: &Leaves, commands: *mut c_void, original: &rsf_game_render_pass) {
        let recording = RECORDING
            .take()
            .filter(|(list, _)| *list == commands)
            .map(|(_, recording)| recording);
        let mut views = self.lock_views();
        let settings = self.lock().settings;
        let mut pass = *original;
        pass.screen = if settings.gameplay != 0 {
            RSF_SCREEN_FLIGHT
        } else {
            RSF_SCREEN_MENU
        };
        if pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0 {
            self.reconfigured();
            destroy_views(&mut views, leaves);
        }
        for sr in views.reap(pass.view_key, pass.source_frame_id) {
            if !sr.is_null() {
                leaves.sr_destroy(sr);
            }
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
                None => {
                    view.seen(pass.source_frame_id);
                    return;
                }
            }
        }
        view.seen(pass.source_frame_id);
        if pass.camera_valid == 0 || self.lock().pending {
            return;
        }
        let Some(slot) = view.slot(commands) else {
            return;
        };
        view.record(recording);
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
        let backend = if settings.sr_backend == RSF_SR_SPATIAL {
            RSF_SR_NONE
        } else {
            settings.sr_backend
        };
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
            ..Default::default()
        };
        let identity = self.presentation.identity();
        identity.lend(&mut setup);
        let (created, sr) = leaves.sr_create(commands, &mut setup)?;
        self.lock().status.sr_result = created;
        let backend = setup.backend;
        let render = (setup.render_width, setup.render_height);
        if sr.is_null()
            || render.0 == 0
            || render.1 == 0
            || render.0 > output.0
            || render.1 > output.1
        {
            self.temporal_ready.store(false, SeqCst);
            let mut state = self.lock();
            state.plan = EMPTY_PLAN;
            state.status.active_sr = 0;
            if !sr.is_null() {
                state.status.sr_result = backend_result::FEATURE_FAILED;
                drop(state);
                leaves.sr_destroy(sr);
            }
            return None;
        }
        let mut view = View::created(sr, backend, output);
        view.render = render;
        self.view_planned(backend, output, view.render);
        self.log(
            leaves,
            &format!(
                "URP SR requested={} active={backend} result={created} {}x{} -> {}x{}",
                settings.sr_backend, view.render.0, view.render.1, output.0, output.1
            ),
        );
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
unsafe extern "C" fn rsfu_configure(input: *const rsf_unity_package_settings) -> c_int {
    let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(leaves) = loaded() else {
        return 0;
    };
    if input.is_null()
        || unsafe { struct_size(input) } != size_of::<rsf_unity_package_settings>() as u32
    {
        return 0;
    }
    c_int::from(PACKAGE.configure(&unsafe { input.read() }, leaves))
}

/// Accepts a longer structure and writes only this version's fields.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_settings(output: *mut rsf_unity_package_settings) -> c_int {
    let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
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
unsafe extern "C" fn rsfu_status(output: *mut rsf_unity_package_status) -> c_int {
    let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(leaves) = loaded() else {
        return 0;
    };
    if output.is_null()
        || unsafe { struct_size(output) } != size_of::<rsf_unity_package_status>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.status(leaves)) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_frame_counters(output: *mut rsf_unity_frame_counters) -> c_int {
    let Some(leaves) = loaded() else {
        return 0;
    };
    if output.is_null()
        || unsafe { struct_size(output) } != size_of::<rsf_unity_frame_counters>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.frame_counters(leaves)) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsfu_temporal_ready() -> c_int {
    c_int::from(PACKAGE.temporal_ready.load(SeqCst))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfu_initialize(
    runtime: *const c_char,
    engine: *const c_char,
    project: *const c_char,
    output: *mut rsf_unity_native_api,
) -> c_int {
    let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(leaves) = loaded() else {
        return code::FAILED;
    };
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
    match PACKAGE.initialize(leaves, identity) {
        Ok(api) => {
            unsafe { output.write(api) };
            code::STARTED
        }
        Err(result) => result,
    }
}

unsafe extern "C" fn render_config(_: *mut c_void, output: *mut rsf_game_render_config) -> c_int {
    if output.is_null()
        || unsafe { struct_size(output) } < size_of::<rsf_game_render_config>() as u32
    {
        return 0;
    }
    unsafe { output.write(PACKAGE.lock().plan) };
    1
}

pub(crate) extern "C" fn rsf_unity_package_bridge_failed(result: i32) {
    PACKAGE.bridge_failed(result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::RSF_SR_DLSS;
    use crate::sys::rsf_fg_status;
    use crate::sys::rsf_fg12_setup;
    use rsf_game_sys::{rsf_game_cpu_event, rsf_game_host_services};
    use std::cell::{Cell, RefCell};
    use std::ffi::CStr;
    use std::ptr::without_provenance_mut;

    #[derive(Clone, Copy)]
    struct Fake {
        sharpening_valid: bool,
        preset_valid: bool,
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
        /// Auto's backend for the adapter; zero when it cannot be read.
        adapter: u32,
        /// The command list has a device.
        device: bool,
        /// A vendor backend creates a context; backend 0 always does.
        vendor_creates: bool,
        create_result: i32,
        evaluate_result: i32,
        plan_result: i32,
        /// A context's planned input extents.
        plan: (u32, u32),
        install_result: i32,
        /// The presentation host has an owner.
        owner: bool,
        /// Auto's generation provider for Unity's adapter; None when it cannot be read.
        fg_adapter: Option<u32>,
        request_result: i32,
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

    const READY: Fake = Fake {
        sharpening_valid: true,
        preset_valid: true,
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
        adapter: 3,
        device: true,
        vendor_creates: true,
        create_result: 0,
        evaluate_result: 0,
        plan_result: 0,
        plan: (1280, 720),
        install_result: 0,
        owner: true,
        fg_adapter: Some(3),
        request_result: 0,
        shown: true,
        preview_ready: true,
        preview_owns: true,
        vulkan: false,
        surface_owns: true,
        class: 1,
        registry: true,
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

    pub(super) unsafe fn sharpening_valid(_: *const rsf_sharpening_options) -> u32 {
        call("sharpening".into());
        fake().sharpening_valid.into()
    }
    pub(super) unsafe fn dlss_preset_valid(_: u32) -> u32 {
        call("preset".into());
        fake().preset_valid.into()
    }
    pub(super) fn set_engine_spatial(spatial: u32, quality: u32) -> c_int {
        call(format!("spatial {spatial} {quality}"));
        1
    }
    pub(super) unsafe fn fg_options(options: *const rsf_native_fg_options) {
        let o = unsafe { *options };
        call(format!(
            "options {} {} {} {} {} {}",
            o.struct_size, o.mode, o.generated_frames, o.reflex_mode, o.frame_limit_us, o.debug
        ));
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
    pub(super) fn bridge_try_close() -> u32 {
        assert!(!fake().close_held);
        call("close".into());
        set(|f| f.close_held = f.drained);
        fake().close_held.into()
    }
    pub(super) unsafe fn end_session(session: u64) -> c_int {
        assert!(fake().close_held);
        call(format!("end {session}"));
        fake().ended.into()
    }
    pub(super) unsafe fn sr_quality_supported(backend: u32, quality: u32) -> u32 {
        u32::from(backend != RSF_SR_DLSS || quality != ULTRA_QUALITY)
    }
    pub(super) unsafe fn sr_create(
        _: *mut c_void,
        setup: *mut rsf_unity_package_view_setup,
        result: *mut i32,
        output: *mut *mut rsf_sr12,
    ) -> u32 {
        let mut s = unsafe { *setup };
        let f = fake();
        if s.backend == RSF_SR_AUTO {
            call("default".into());
            if f.adapter == 0 {
                return 0;
            }
            s.backend = f.adapter;
        }
        // The identity is always lent; a loaded one follows the depth flag.
        let text = |value: *const c_char| unsafe { CStr::from_ptr(value) }.to_str().unwrap();
        let identity = match text(s.root_utf8) {
            "" => String::new(),
            root => format!(
                " {root} {} {} {} {} {} {}",
                text(s.streamline_utf8),
                text(s.interposer_utf8),
                text(s.fsr_utf8),
                text(s.xess_utf8),
                text(s.engine_version_utf8),
                text(s.project_id_utf8)
            ),
        };
        let create = |backend| {
            call(format!(
                "create {backend} {} {} {}x{} {}x{} {}{identity}",
                s.quality,
                s.dlss_preset,
                s.output_width,
                s.output_height,
                s.render_width,
                s.render_height,
                s.inverted_depth
            ));
        };
        create(s.backend);
        if !f.device {
            return 0;
        }
        if s.backend != 0 && !f.vendor_creates {
            create(0);
            s.backend = 0;
        }
        call("plan".into());
        let context = without_provenance_mut(CONTEXT + s.backend as usize);
        let result_code = if f.plan_result == 0 {
            (s.render_width, s.render_height) = f.plan;
            f.create_result
        } else {
            unsafe { sr_destroy(context) };
            f.plan_result
        };
        let context = if f.plan_result == 0 {
            context
        } else {
            null_mut()
        };
        unsafe {
            setup.write(s);
            result.write(result_code);
            output.write(context);
        }
        1
    }
    pub(super) unsafe fn sharpening_defaults(output: *mut rsf_sharpening_options) {
        call("defaults".into());
        unsafe { (*output).struct_size = 56 };
    }
    pub(super) unsafe fn sr_set_sharpening(
        _: *mut rsf_sr12,
        options: *const rsf_sharpening_options,
    ) -> i32 {
        call(format!("sharpen {}", unsafe { (*options).struct_size }));
        -1
    }
    pub(super) unsafe fn sr_evaluate(
        sr: *mut rsf_sr12,
        _: *mut c_void,
        pass: *const rsf_game_render_pass,
        slot: u32,
    ) -> i32 {
        let screen = unsafe { (*pass).screen };
        call(format!("evaluate {:#x} {slot} {screen}", sr as usize));
        fake().evaluate_result
    }
    pub(super) unsafe fn sr_destroy(sr: *mut rsf_sr12) {
        call(format!("destroy {:#x}", sr as usize));
    }
    pub(super) fn bridge_finish_retirement(commit: u32) {
        assert!(fake().close_held);
        set(|f| f.close_held = false);
        call(if commit == 0 { "retain" } else { "release" }.into());
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
    pub(super) unsafe fn present_has_owner() -> c_int {
        call("owner".into());
        fake().owner.into()
    }
    pub(super) unsafe fn present_request(backend: u32) -> i32 {
        call(format!("request {backend}"));
        fake().request_result
    }
    pub(super) unsafe fn fg_default_backend(backend: *mut u32) -> u32 {
        call("fg default".into());
        let Some(adapter) = fake().fg_adapter else {
            return 0;
        };
        unsafe { backend.write(adapter) };
        1
    }
    pub(super) fn bridge_open(editor: u32) -> u32 {
        assert!(!fake().close_held);
        if editor != 0 {
            return 1;
        }
        call("surface".into());
        fake().surface.into()
    }
    pub(super) fn session_seed() -> u64 {
        call("seed".into());
        41
    }
    /// Checks the host services and writes the API a bridge writes on success.
    pub(super) unsafe fn bridge_prepare(
        services: *const rsf_game_host_services,
        api: *mut rsf_unity_native_api,
    ) -> u32 {
        let s = unsafe { *services };
        call(format!("prepare {}", s.session_id));
        assert_eq!((s.struct_size, s.abi_version), (104, 24));
        assert!(s.user.is_null());
        assert!(
            s.render_pass.map(|f| f as usize)
                == Some(routing::render_callback as *const () as usize)
        );
        assert!(s.render_config.map(|f| f as usize) == Some(render_config as *const () as usize));
        assert!(s.log.is_some() && s.cpu_event.is_some());
        assert!(s.scope_state.is_none() && s.process_upscale.is_none() && s.submit.is_none());
        assert!(s.present.is_none() && s.process_latency.is_none() && s.render_feedback.is_none());
        if !fake().prepared {
            return 0;
        }
        unsafe {
            (*api).struct_size = 72;
            (*api).session_id = s.session_id;
        }
        1
    }
    fn wide(path: *const u16) -> String {
        let length = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
        String::from_utf16(unsafe { std::slice::from_raw_parts(path, length) }).unwrap()
    }
    fn handle(value: usize) -> *mut c_void {
        without_provenance_mut(value)
    }
    pub(super) unsafe fn local_app_data(output: *mut u16, capacity: u32) -> u32 {
        call("local".into());
        assert_eq!(capacity, 32768);
        let Some(local) = fake().local else {
            return 0;
        };
        let local: Vec<u16> = local.encode_utf16().collect();
        unsafe { output.copy_from_nonoverlapping(local.as_ptr(), local.len()) };
        local.len() as u32
    }
    pub(super) unsafe fn create_directory(path: *const u16) {
        call(format!("mkdir {}", wide(path)));
    }
    pub(super) unsafe fn log_open(path: *const u16) -> *mut c_void {
        call(format!("open {}", wide(path)));
        handle(fake().file)
    }
    pub(super) unsafe fn log_write(file: *mut c_void, text: *const c_char) {
        let text = unsafe { CStr::from_ptr(text) }.to_str().unwrap();
        call(if file.is_null() {
            format!("log {text}")
        } else {
            format!("log {:#x} {text}", file as usize)
        });
    }
    pub(super) unsafe fn log_close(file: *mut c_void) {
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
    pub(super) fn subclass(window: *mut c_void) -> u32 {
        call(format!("subclass {:#x}", window as usize));
        fake().subclass_accepted.into()
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
    pub(super) unsafe fn timestamp(counter: *mut u64, frequency: *mut u64) {
        unsafe {
            counter.write(1000);
            frequency.write(10);
        }
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
    pub(super) unsafe fn fg_submitted(session: u64, source_frame: u64) {
        call(format!("submitted {session} {source_frame}"));
    }
    pub(super) unsafe fn fg_window(pass: *const rsf_game_render_pass) {
        call(format!("window {}", unsafe { (*pass).view_key }));
    }
    pub(super) unsafe fn fg_hudless(commands: *mut c_void, pass: *const rsf_game_render_pass) {
        call(format!("hudless {:#x} {}", commands as usize, unsafe {
            (*pass).view_key
        }));
    }
    pub(super) unsafe fn preview_present(
        resource: *mut c_void,
        pass: *const rsf_game_render_pass,
        common: u32,
    ) -> u32 {
        assert_eq!(resource, unsafe { (*pass).color_input });
        call(format!("present {:#x} {common}", resource as usize));
        fake().shown.into()
    }
    pub(super) unsafe fn texture_desc(
        resource: *mut c_void,
        width: *mut u64,
        height: *mut u32,
        format: *mut u32,
    ) {
        call(format!("desc {:#x}", resource as usize));
        unsafe {
            width.write(640);
            height.write(360);
            format.write(10);
        }
    }
    pub(super) fn preview_prepare(width: u32, height: u32) -> u32 {
        call(format!("preview prepare {width}x{height}"));
        fake().preview_ready.into()
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
    pub(super) unsafe fn bridge_load(_: *mut c_void) -> u32 {
        call("bridge load".into());
        fake().registry.into()
    }
    pub(super) fn bridge_unload() {
        call("bridge unload".into());
    }

    /// Fresh fakes and an empty call record for this test.
    fn leaves() -> Leaves {
        FAKE.with(|cell| cell.set(READY));
        calls();
        Leaves
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
        assert_eq!(
            (state.status.struct_size, state.status.abi_version),
            (184, 1)
        );
        assert_eq!((state.plan.struct_size, state.plan.enabled), (24, 0));
        assert!(!state.pending && state.session == Session::Unopened);
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
    fn runtime_admits_explicit_quality_and_defers_auto() {
        let package = Package::new();
        let leaves = leaves();
        let mut input = DEFAULT_SETTINGS;
        input.quality = ULTRA_QUALITY;
        input.sr_backend = RSF_SR_DLSS;
        assert!(!package.configure(&input, &leaves));
        input.sr_backend = RSF_SR_AUTO;
        package.lock().status.active_sr = RSF_SR_DLSS;
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
        package.evaluated(backend_result::OK, 0);
        let status = package.lock().status;
        assert_eq!(
            (status.sr_result, status.active_sr, status.evaluated_frames),
            (4, 0, 1)
        );
        package.evaluated(backend_result::OK, 3);
        assert_eq!(package.lock().status.active_sr, 3);
        assert_eq!(package.lock().status.sr_result, backend_result::OK);
        package.evaluated(-2, 0);
        let status = package.lock().status;
        assert_eq!(
            (status.sr_result, status.refused_frames, status.active_sr),
            (-2, 1, 3)
        );
        package.lock().settings.sr_backend = RSF_SR_SPATIAL;
        package.evaluated(backend_result::OK, 0);
        let status = package.lock().status;
        assert_eq!(
            (status.sr_result, status.active_sr),
            (backend_result::OK, RSF_SR_SPATIAL)
        );
        package.lock().settings.sr_backend = RSF_SR_NONE;
        package.lock().status.sr_result = 4;
        package.evaluated(backend_result::OK, 0);
        assert_eq!(package.lock().status.sr_result, backend_result::OK);
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
        assert_eq!((status.derived_boundaries, status.active_sr), (8, 3));
        // The aggregate gives a provider-switch failure precedence over a session failure.
        set(|f| f.last_result = -3);
        set(|f| f.latency = false);
        package.bridge_failed(-9);
        package.lock().status.sleep_calls = 1;
        let status = package.status(&leaves);
        assert_eq!(calls(), ["fg status", "backend", "switch", "latency"]);
        assert_eq!(
            (status.fg_result, status.sr_result, status.active_sr),
            (-7, -9, 0)
        );
        assert_eq!(status.sleep_calls, 0);
        assert_eq!((status.latency_available, status.latency_mode), (0, 0));
        assert_eq!((status.latency_owner, status.keyboard_inputs), (0, 0));
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

    /// open's calls in the player: the first session opens the process log and hooks the player
    /// window, every session checks the Vulkan surface and prepares the bridge.
    fn opened(first: bool) -> Vec<String> {
        let mut calls = Vec::new();
        if first {
            let log = format!("open L\\ReScaleFrame\\Unity\\{}.log", std::process::id());
            calls.extend([
                "local".into(),
                "mkdir L\\ReScaleFrame".into(),
                "mkdir L\\ReScaleFrame\\Unity".into(),
                log,
                "player".into(),
                "subclass 0x20".into(),
            ]);
        }
        calls.extend(["surface", "seed", "prepare 42"].map(String::from));
        calls
    }
    fn started(package: &Package, leaves: &Leaves, identity: Identity) -> Result<u64, i32> {
        package
            .initialize(leaves, identity)
            .map(|api| api.session_id)
    }

    #[test]
    fn sessions_start_retire_and_refuse_while_busy() {
        let package = Package::new();
        let leaves = leaves();
        package.lock().settings.quality = 2;
        package.bridge_failed(-4);
        let session = || Identity::new(b"R", b"6000", b"P");
        // The bridge's API, with the session id of the host services, is handed out.
        assert_eq!(started(&package, &leaves, session()), Ok(42));
        assert_eq!(
            calls(),
            [
                opened(true),
                vec!["spatial 0 2".into(), "activate 1".into()]
            ]
            .concat()
        );
        assert_eq!(package.services.session(), 42);
        assert_eq!(package.bridge_result.load(SeqCst), 0);
        assert!(package.lock().session == Session::Started(42));
        assert_eq!(package.presentation.identity().xess.to_bytes(), b"R/xess");
        // A started session only restarts once drained and its own session ended.
        package.view_planned(3, (8, 8), (4, 4));
        package.lock_views().entry(1).sr = without_provenance_mut(0x300);
        package.lock().status.evaluated_frames = 5;
        set(|f| f.drained = false);
        // A busy restart keeps the running session's identity.
        assert_eq!(
            started(&package, &leaves, Identity::new(b"Q", b"1", b"P")),
            Err(code::BUSY)
        );
        assert_eq!(calls(), ["close"]);
        assert_eq!(package.presentation.identity().root.to_bytes(), b"R");
        set(|f| f.drained = true);
        set(|f| f.ended = false);
        assert_eq!(started(&package, &leaves, session()), Err(code::BUSY));
        assert_eq!(calls(), ["close", "end 42", "retain"]);
        assert!(package.temporal_ready.load(SeqCst));
        assert_eq!(package.services.session(), 42);
        set(|f| f.ended = true);
        set(|f| f.prepared = false);
        assert_eq!(started(&package, &leaves, session()), Err(code::FAILED));
        // The log and the input window outlive the session.
        assert_eq!(
            calls(),
            [
                vec![
                    "close".into(),
                    "end 42".into(),
                    "destroy 0x300".into(),
                    "release".into()
                ],
                opened(false)
            ]
            .concat()
        );
        {
            let state = package.lock();
            assert!(state.session == Session::Opening);
            assert_eq!((state.status.evaluated_frames, state.plan.enabled), (0, 0));
        }
        assert!(!package.temporal_ready.load(SeqCst));
        assert_eq!(package.services.session(), 0);
        set(|f| f.drained = false);
        assert_eq!(started(&package, &leaves, session()), Err(code::BUSY));
        assert_eq!(calls(), ["close"]);
        set(|f| f.drained = true);
        set(|f| f.prepared = true);
        package.lock().settings.sr_backend = RSF_SR_SPATIAL;
        assert_eq!(started(&package, &leaves, session()), Ok(42));
        assert_eq!(
            calls(),
            [
                vec!["close".into(), "release".into()],
                opened(false),
                vec!["spatial 1 2".into(), "activate 1".into()]
            ]
            .concat()
        );
        assert_eq!(package.lock().status.requested_sr, RSF_SR_SPATIAL);
        // Shutdown and device shutdown use the same checked transaction as restart.
        set(|f| f.drained = false);
        assert!(!package.retire(&leaves));
        assert_eq!(calls(), ["close"]);
        set(|f| f.drained = true);
        assert!(package.retire(&leaves));
        assert_eq!(calls(), ["close", "end 42", "release"]);
        assert!(package.lock().session == Session::Stopped);
        set(|f| f.drained = false);
        assert!(package.retire(&leaves));
        assert!(calls().is_empty());
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
    fn a_view_is_created_with_the_package_identity() {
        let package = Package::new();
        let leaves = leaves();
        package
            .presentation
            .set_identity(Identity::new(b"R", b"6000", b"P"));
        package.render(&leaves, list(0xa0), &sr_pass(1, (8, 8), (4, 4)));
        assert_eq!(
            calls()[1],
            "create 3 1 0 8x8 4x4 1 R R/streamline R/streamline/sl.interposer.dll R/fidelityfx \
             R/xess 6000 P"
        );
    }

    #[test]
    fn generation_installs_once_when_identified() {
        let package = Package::new();
        let leaves = leaves();
        package.install(&leaves);
        assert!(calls().is_empty());
        package
            .presentation
            .set_identity(Identity::new(b"R", b"6000", b"P"));
        set(|f| f.install_result = -4);
        package.install(&leaves);
        // Auto, unlimited, generation off with Reflex off until rsfu_configure, inverted finite
        // depth in metres, switchable, HUD-less backbuffer, Unity's Streamline identity.
        assert_eq!(
            calls(),
            [
                "install 152 7 4294967295 R/fidelityfx | 24 0 1 0 0 0 | 1 0 1 1 4 | \
                 R/fidelityfx R/fidelityfx R/xess R/streamline | 2 6000 P"
            ]
        );
        assert_eq!(package.lock().status.fg_result, -4);
        // A refusal is retried; an accepted install is not repeated.
        set(|f| f.install_result = 0);
        package.install(&leaves);
        assert_eq!(calls().len(), 1);
        assert_eq!(package.lock().status.fg_result, 0);
        package.lock().status.fg_result = 9;
        package.install(&leaves);
        assert!(calls().is_empty());
        assert_eq!(package.lock().status.fg_result, 9);
    }

    #[test]
    fn the_window_pass_requests_the_stored_provider() {
        let package = Package::new();
        let leaves = leaves();
        set(|f| f.owner = false);
        package.request_generation(&leaves);
        assert_eq!(calls(), ["owner"]);
        set(|f| f.owner = true);
        // Auto waits for a readable adapter.
        set(|f| f.fg_adapter = None);
        package.request_generation(&leaves);
        assert_eq!(calls(), ["owner", "fg default"]);
        // Auto resolves once; the active provider (5) is not requested again.
        set(|f| f.fg_adapter = Some(5));
        package.request_generation(&leaves);
        assert_eq!(calls(), ["owner", "fg default", "backend"]);
        set(|f| f.fg_adapter = Some(3));
        package.request_generation(&leaves);
        assert_eq!(calls(), ["owner", "backend"]);
        assert_eq!(package.lock().status.fg_result, 0);
        package.device_event(&leaves, true);
        package.device_event(&leaves, false);
        package.device_event(&leaves, true);
        calls();
        package.request_generation(&leaves);
        assert_eq!(calls(), ["owner", "fg default", "backend", "request 3"]);
        // Another provider is requested and its result recorded.
        package.lock().settings.fg_backend = RSF_FG_BACKEND_DLSS;
        set(|f| f.request_result = -2);
        package.request_generation(&leaves);
        assert_eq!(calls(), ["owner", "backend", "request 1"]);
        assert_eq!(package.lock().status.fg_result, -2);
        // An adapter without a provider resolves Auto to off, read again every pass.
        let package = Package::new();
        set(|f| f.fg_adapter = Some(0));
        for _ in 0..2 {
            package.request_generation(&leaves);
            assert_eq!(calls(), ["owner", "fg default", "backend", "request 0"]);
        }
    }

    #[test]
    fn a_refused_or_invalid_plan_releases_the_context_without_publishing() {
        let package = Package::new();
        let leaves = leaves();
        let pass = sr_pass(1, (1920, 1080), (960, 540));
        for (result, extent, expected) in [
            (-4, (960, 540), -4),
            (0, (0, 540), backend_result::FEATURE_FAILED),
            (0, (1921, 540), backend_result::FEATURE_FAILED),
        ] {
            set(|f| {
                f.plan_result = result;
                f.plan = extent;
            });
            package.render(&leaves, list(0xa0), &pass);
            assert_eq!(&calls()[2..], ["plan", "destroy 0x103"]);
            assert!(!package.temporal_ready.load(SeqCst));
            let state = package.lock();
            assert_eq!(state.plan.enabled, 0);
            assert_eq!(state.status.sr_result, expected);
        }
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
        let mut plan = rsf_game_render_config {
            struct_size: 23,
            ..EMPTY_PLAN
        };
        assert_eq!(unsafe { render_config(null_mut(), &mut plan) }, 0);
        plan.struct_size = 24;
        assert_eq!(unsafe { render_config(null_mut(), &mut plan) }, 1);
        calls();
        let pass = sr_pass(1, (8, 8), (4, 4));
        unsafe {
            routing::render_callback(null_mut(), null_mut(), &pass, 1);
            routing::render_callback(null_mut(), list(1), std::ptr::null(), 1);
            routing::render_callback(null_mut(), list(1), &pass, 1);
        }
        assert!(calls().is_empty());
        let (text, none) = (c"x".as_ptr(), std::ptr::null());
        // SAFETY: integers, nullable function pointers and a nullable pointer.
        let mut api: rsf_unity_native_api = unsafe { std::mem::zeroed() };
        let output = &raw mut api;
        unsafe {
            assert_eq!(rsfu_initialize(text, text, text, output), code::FAILED);
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
        let leaves = leaves();
        package
            .presentation
            .set_identity(Identity::new(b"R", b"6000", b"P"));
        package.loaded(&leaves, false, &wide_text("C:\\rsf.log"));
        let loaded = calls();
        assert_eq!(loaded[0], "open C:\\rsf.log");
        assert!(loaded[1].starts_with("install 152 7 "));
        assert_eq!(loaded.len(), 2);
        // The configured log stands in for the process log; lines reach both outputs.
        package.log(&leaves, "hello");
        assert_eq!(calls(), ["log 0x10 hello"]);
        assert!(package.open(&leaves));
        assert_eq!(
            calls(),
            ["player", "subclass 0x20", "surface", "seed", "prepare 42"]
        );
        // Unload restores the procedure and closes the log; later lines reach the debugger only.
        package.unload(&leaves);
        assert_eq!(calls(), ["restore 0x20", "close 0x10"]);
        package.log(&leaves, "late");
        assert_eq!(calls(), ["log late"]);
        package.unload(&leaves);
        assert!(calls().is_empty());
        // The player has no preview input.
        package.preview_input(&leaves);
        assert!(calls().is_empty());
    }

    #[test]
    fn the_editor_installs_per_session_and_hooks_the_preview() {
        let package = Package::new();
        let leaves = leaves();
        package
            .presentation
            .set_identity(Identity::new(b"R", b"6000", b"P"));
        package.loaded(&leaves, true, &[]);
        assert!(calls().is_empty());
        // No player window and no Vulkan surface: the preview presents in the Editor.
        assert!(package.open(&leaves));
        let opened = calls();
        assert_eq!(opened[0], "local");
        assert!(opened[4].starts_with("install 152 7 "));
        assert_eq!(opened[5..], ["seed", "prepare 42"]);
        // The preview's parent is hooked once it exists, then kept.
        set(|f| f.parent = 0);
        package.preview_input(&leaves);
        assert_eq!(calls(), ["parent"]);
        set(|f| f.parent = 0x40);
        package.preview_input(&leaves);
        assert_eq!(calls(), ["parent", "subclass 0x40"]);
        package.preview_input(&leaves);
        assert!(calls().is_empty());
        assert!(package.open(&leaves));
        assert_eq!(calls(), ["seed", "prepare 42"]);
    }

    #[test]
    fn a_refused_window_subclass_is_retried() {
        let package = Package::new();
        let leaves = leaves();
        package.loaded(&leaves, true, &[]);
        set(|f| f.subclass_accepted = false);
        package.preview_input(&leaves);
        assert_eq!(calls(), ["parent", "subclass 0x40"]);
        set(|f| f.subclass_accepted = true);
        package.preview_input(&leaves);
        assert_eq!(calls(), ["parent", "subclass 0x40"]);
        package.preview_input(&leaves);
        assert!(calls().is_empty());
    }

    #[test]
    fn a_refused_log_or_surface_is_retried_by_the_next_session() {
        let package = Package::new();
        let leaves = leaves();
        set(|f| f.file = 0);
        package.loaded(&leaves, false, &wide_text("C:\\rsf.log"));
        assert_eq!(calls(), ["open C:\\rsf.log"]);
        set(|f| f.surface = false);
        assert!(!package.open(&leaves));
        let mut refused = opened(true);
        refused.truncate(refused.len() - 2);
        refused.push(
            "log Unity graphics bridge or player presentation surface could not open.".into(),
        );
        assert_eq!(calls(), refused);
        // Without %LOCALAPPDATA% there is no process log; the window stays hooked.
        set(|f| f.surface = true);
        set(|f| f.local = None);
        assert!(package.open(&leaves));
        assert_eq!(calls(), ["local", "surface", "seed", "prepare 42"]);
    }

    #[test]
    fn received_input_is_keyed_to_the_cpu_frame_in_progress() {
        use rsf_game_sys::{RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END};
        let package = Package::new();
        let leaves = leaves();
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
        package.window_message(&leaves, 0x0100, 0);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 1 0x100 21 1"]);
        package.window_message(&leaves, 0x000f, 0);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 0 0xf 5 1"]);
        // The bridge's events reach the Runtime unchanged.
        package.cpu_event(&leaves, &frame(RSF_GAME_CPU_FRAME_BEGIN, 7));
        assert_eq!(calls(), ["cpu 0 42 7 5/10 0 0x0 0 1"]);
        package.window_message(&leaves, 0x020e, 0);
        assert_eq!(calls(), ["cpu 5 42 7 1000/10 2 0x20e 5 1"]);
        // WM_INPUT carries its raw device's kind.
        for (raw, kind) in [(Some(1), 1), (Some(0), 2), (Some(2), 0), (None, 0)] {
            set(|f| f.raw = raw);
            package.window_message(&leaves, 0x00ff, 9);
            assert_eq!(
                calls(),
                [
                    "raw 9".into(),
                    format!("cpu 5 42 7 1000/10 {kind} 0xff 5 1")
                ]
            );
        }
        // Managed input is keyed to its frame only while that frame is in progress.
        assert_eq!(package.input(&leaves, 7, 3), backend_result::OK);
        assert_eq!(calls(), ["cpu 5 42 7 1000/10 3 0x0 1 1"]);
        package.input(&leaves, 6, 4);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 4 0x0 17 1"]);
        package.cpu_event(&leaves, &frame(RSF_GAME_CPU_FRAME_END, 7));
        assert_eq!(calls(), ["cpu 4 42 7 5/10 0 0x0 0 1"]);
        package.input(&leaves, 7, 1);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 1 0x0 17 1"]);
        package.window_message(&leaves, 0x0109, 0);
        assert_eq!(calls(), ["cpu 5 42 0 1000/10 1 0x109 21 1"]);
    }

    #[test]
    fn the_render_callback_routes_each_pass() {
        use rsf_game_sys::{
            RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW,
        };
        let package = Package::new();
        let leaves = leaves();
        set(|f| f.owner = false);
        let mut pass = sr_pass(3, (8, 8), (4, 4));
        (pass.role, pass.session_id, pass.source_frame_id) = (RSF_GAME_RENDER_SR, 42, 7);
        // An end reports every pass but the window's as submitted.
        assert!(!package.route(&leaves, list(1), &pass, false));
        assert_eq!(calls(), ["submitted 42 7"]);
        pass.role = RSF_GAME_RENDER_WINDOW;
        assert!(!package.route(&leaves, list(1), &pass, false));
        assert!(calls().is_empty());
        // The player's window pass asks for the provider, then hands the pass to generation.
        assert!(!package.route(&leaves, list(1), &pass, true));
        assert_eq!(calls(), ["owner", "window 3"]);
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
        assert!(!package.route(&leaves, list(1), &pass, true));
        assert_eq!(calls(), ["hudless 0x1 3"]);
        // Any other pass goes to the SR pass; the player has no preview to prepare.
        pass.role = RSF_GAME_RENDER_SR;
        assert!(package.route(&leaves, list(1), &pass, true));
        assert!(calls().is_empty());
    }

    #[test]
    fn the_editor_preview_is_admitted_and_its_refusals_budgeted() {
        use rsf_game_sys::{RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW};
        let package = Package::new();
        let leaves = leaves();
        set(|f| {
            f.owner = false;
            f.shown = false;
            f.preview_ready = false;
        });
        package.services.editor.store(true, SeqCst);
        let mut pass = sr_pass(3, (8, 6), (4, 3));
        pass.role = RSF_GAME_RENDER_SR;
        // The SR pass waits for the preview's resources.
        assert!(!package.route(&leaves, list(1), &pass, true));
        assert_eq!(calls(), ["preview prepare 8x6"]);
        set(|f| f.preview_ready = true);
        assert!(package.route(&leaves, list(1), &pass, true));
        assert_eq!(calls(), ["preview prepare 8x6"]);
        // A refusal without a texture still spends the budget.
        pass.role = RSF_GAME_RENDER_WINDOW;
        assert!(!package.route(&leaves, list(1), &pass, true));
        assert_eq!(calls(), ["owner", "present 0x0 0"]);
        // Route the D3D11 producer's real Editor plan after its native resource handoff.
        use crate::{d3d11_pass, sys::rsf_unity_packet};
        let mut packet = unsafe { std::mem::zeroed::<rsf_unity_packet>() };
        packet.camera = pass.camera;
        packet.color = without_provenance_mut(0x50);
        let mut pass = d3d11_pass::plan(d3d11_pass::EVENT_WINDOW, &packet, true).pass;
        pass.color_input = packet.color;
        let logged = "log Editor presentation pending: texture=640x360 format=10 expected=8x6";
        for _ in 0..3 {
            package.route(&leaves, list(1), &pass, true);
            assert_eq!(calls(), ["owner", "present 0x50 1", "desc 0x50", logged]);
        }
        package.route(&leaves, list(1), &pass, true);
        assert_eq!(calls(), ["owner", "present 0x50 1"]);
        // A shown preview neither logs nor counts.
        set(|f| f.shown = true);
        package.route(&leaves, list(1), &pass, true);
        assert_eq!(calls(), ["owner", "present 0x50 1"]);
        assert_eq!(package.preview_reports.load(SeqCst), 5);
    }

    #[test]
    fn the_window_guard_follows_the_host() {
        let package = Package::new();
        let leaves = leaves();
        let window = without_provenance_mut(0x60);
        // The D3D12 player accepts its own UnityWndClass window, and another window of this
        // process only when it is the Vulkan surface.
        assert!(package.accept_window(&leaves, window));
        assert_eq!(calls(), ["vulkan renderer", "class 0x60"]);
        set(|f| {
            f.class = 2;
            f.surface_owns = false;
        });
        assert!(!package.accept_window(&leaves, window));
        assert_eq!(
            calls(),
            ["vulkan renderer", "class 0x60", "surface owns 0x60"]
        );
        set(|f| f.class = 0);
        assert!(!package.accept_window(&leaves, window));
        assert_eq!(calls(), ["vulkan renderer", "class 0x60"]);
        // The Vulkan player accepts only its surface.
        set(|f| {
            f.vulkan = true;
            f.surface_owns = true;
        });
        assert!(package.accept_window(&leaves, window));
        assert_eq!(calls(), ["vulkan renderer", "surface owns 0x60"]);
        // The Editor accepts only its preview.
        package.services.editor.store(true, SeqCst);
        set(|f| f.preview_owns = false);
        assert!(!package.accept_window(&leaves, window));
        assert_eq!(calls(), ["preview owns 0x60"]);
    }

    #[test]
    fn the_device_lifecycle_loads_retires_and_retains() {
        let package = Package::new();
        let leaves = leaves();
        // Waiting: nothing to retire, and the bridge loads once Unity's interfaces exist.
        assert!(package.shutdown(&leaves));
        package.device_event(&leaves, false);
        assert!(calls().is_empty());
        set(|f| f.registry = false);
        package.device_event(&leaves, true);
        assert_eq!(calls(), ["bridge load"]);
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_WAITING);
        set(|f| f.registry = true);
        package.device_event(&leaves, true);
        package.device_event(&leaves, true);
        assert_eq!(calls(), ["bridge load"]);
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_READY);
        // Ready: a shutdown retires the session, then unloads the bridge.
        package.device_event(&leaves, false);
        assert_eq!(calls(), ["close", "release", "bridge unload"]);
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_WAITING);
        // A refused retirement retains the device for the process.
        package.device_event(&leaves, true);
        package.lock().session = Session::Opening;
        set(|f| f.drained = false);
        calls();
        package.device_event(&leaves, false);
        assert_eq!(
            calls(),
            [
                "close",
                "log Unity device shutdown retained unproved bridge work; initialization requires a process restart."
            ]
        );
        assert_eq!(package.device.load(SeqCst), routing::DEVICE_RETAINED);
        assert!(!package.shutdown(&leaves));
        package.device_event(&leaves, true);
        package.device_event(&leaves, false);
        assert!(calls().is_empty());
    }
}
