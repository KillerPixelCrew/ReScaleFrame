// SPDX-License-Identifier: MIT
//! The D3D12 host entry behind callback_render_host.h, for an adapter whose render callbacks
//! guarantee command-slot retirement and that may report its engine's own spatial scaler.
//!
//! The adapter starts it with its plugin, probe, saved SR choice and engine identity. Rust owns
//! admission, the refusal codes, the plugin lifecycle order, host state, renderer configuration,
//! scene and submission matching, SR context lifetime, callback recording slots, settings and
//! overlay control. The vendor packages, SR context setup and the overlay's SR card are the
//! parent module's, shared with every D3D12 integration. The adapter's only policy input is
//! `set_engine_spatial`, which learns whether the engine upscales spatially itself, never the
//! vendor; the Runtime decides when the engine's scaler runs.
//!
//! Settings are rsf_render_settings in the preferences file's [Rendering] section, saved on every
//! change like every other host's. A key never saved keeps the adapter's start value: the SR
//! choice, quality and DLSS model it starts the host with, and the generation options it
//! installed generation with. The generation provider stays in fg_choice's key, which the
//! adapter's install reads at the next start.
use super::{Adapter, ResourceDesc, Vendors, sr_card};
use crate::com_ref;
use crate::copy_c_string;
use crate::fg_choice;
use crate::overlay_settings::{fill_sharpening_stats, intent_sharpening};
#[cfg(not(test))]
use crate::plugin_session::Session;
use crate::settings_store::{
    read_settings_and_fg_mode, refused, rsf_settings_store_read_layout, rsf_settings_store_write,
    rsf_settings_store_write_layout, set_sharpening,
};
use crate::sharpening_policy;
use crate::sr_choices::sr_quality;
use crate::sync::lock;
use crate::sys::rsf_sr12_setup as SrSetup;
use crate::sys::{
    RSF_FG_BACKEND_AUTO, RSF_FG_FIXED, RSF_REFLEX_OFF, RSF_RENDER_FRONTEND_ABI_VERSION,
    rsf_render_settings as Settings,
};
use crate::sys::{
    RSF_OVERLAY_SR_CHANGE_APPLIED, RSF_OVERLAY_SR_CHANGE_PENDING, RSF_OVERLAY_SR_CHANGE_REFUSED,
    rsf_overlay_intent as Intent, rsf_overlay_stats as Stats,
};
use crate::sys::{RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_NONE, RSF_SR_SPATIAL, RSF_SR_XESS};
use crate::sys::{
    rsf_native_fg_options as FgOptions, rsf_native_fg_status as FgStatus,
    rsf_sharpening_options as Sharpening,
};
use crate::wide::copy_nul as wide_copy;
use rsf_game_sys::ffi_util::{checked, has_capacity};
use rsf_game_sys::vendor_paths::VendorPaths;
use rsf_game_sys::{
    RSF_CALLBACK_HOST_ABI_VERSION as HOST_ABI_VERSION, RSF_CALLBACK_HOST_ABI_VERSION_V2,
    RSF_CALLBACK_HOST_ABI_VERSION_V3, RSF_GAME_ABI_VERSION, RSF_GAME_RENDER_FG_INPUTS,
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_RESET,
    RSF_GAME_RENDER_SR, RSF_GAME_RENDER_SUBMISSION, RSF_GAME_RENDER_WINDOW,
    recording_slots::RecordingSlots, rsf_callback_host_setup as Setup,
    rsf_callback_host_setup_v2 as SetupV2, rsf_callback_host_setup_v3 as SetupV3,
    rsf_callback_host_status as HostStatus, rsf_game_cpu_event as CpuEvent,
    rsf_game_host_services as Services, rsf_game_render_config as Config,
    rsf_game_render_pass as Pass,
};
use rsf_game_sys::{
    RSF_PLUGIN_SESSION_ABI_VERSION as SESSION_ABI_VERSION,
    rsf_plugin_session_options as SessionOptions,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::File;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows::core::Interface;
use windows_sys::core::{PCWSTR, w};

#[cfg(test)]
mod tests;

type Handle = *mut c_void;
#[cfg(test)]
type Session = c_void;
type SpatialFn = unsafe extern "C" fn(u32, u32) -> c_int;

const OK: i32 = 0;
const BACKEND_OK: i32 = 0;
const BACKEND_NOT_SUPPORTED: i32 = -7;

const SHARPENING_POLL_MS: u64 = 1000;
const SECTION: PCWSTR = w!("Rendering");
// FILE_APPEND_DATA, and FILE_SHARE_READ | FILE_SHARE_WRITE.
const LOG_ACCESS: u32 = 4;
const LOG_SHARE: u32 = 3;

// Production calls the leaves in render_host_native; unit tests call same-named fakes.
#[cfg(not(test))]
use crate::render_host_native as leaf;
#[cfg(test)]
use tests as leaf;

// One engine view's vendor SR context. `serial` is its latest recording on `slot`, zero before one.
struct View {
    key: u64,
    sr: Handle,
    seen: u64,
    slot: u32,
    serial: u64,
}

// A view unseen for this many source frames gives up its context once that context is retired.
const IDLE_SOURCE_FRAMES: u64 = 120;

struct State {
    // A vendor SR context per engine view in first-use order, and the backend-zero context that
    // normalizes generation inputs.
    views: Vec<View>,
    inputs: Handle,
    // Each recording slot's latest recording serial. The adapter retires a slot's previous
    // recording before it records on that slot again, which retires every older recording too.
    slot_serials: [u64; 3],
    serial: u64,
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
    // What is saved: every accepted change, written as one section.
    settings: Settings,
    // The saved generation options are applied once generation reports its start options.
    generation_restored: bool,
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
    // The provider a saved Auto turned into, which the switch must reach before Auto is saved.
    save_target: u32,
    // The owning adapter's PCI vendor, from the last panel frame; zero before one.
    vendor: u32,
}

pub(crate) struct Host {
    state: Mutex<State>,
    log_file: Mutex<File>,
    // Written only under the lifecycle lock; read by stop.
    plugin: AtomicPtr<Session>,
    preferences: Vec<u16>,
    sr_choices: u32,
    vendors: Vendors,
    set_engine_spatial: SpatialFn,
    policy_v2: Option<PolicyV2>,
}

struct PolicyV2 {
    request: unsafe extern "C" fn(u32, u32, *mut u32) -> c_int,
    drained: unsafe extern "C" fn(*mut u32) -> c_int,
    extent: unsafe extern "C" fn(u32, u32, u32, *mut u32, *mut u32) -> c_int,
    spatial_name: CString,
}

fn accepts_backend_quality(backend: u32, quality: u32) -> bool {
    quality < 32 && sr_quality(backend) & (1 << quality) != 0
}

struct Published(*mut Host);
// The host is shared with callbacks through its address; this only moves that address.
unsafe impl Send for Published {}

// The one started host. Start and stop serialize on this lock.
static LIFECYCLE: Mutex<Published> = Mutex::new(Published(null_mut()));

fn try_lifecycle() -> Option<MutexGuard<'static, Published>> {
    // Adapter callbacks may reenter either public lifecycle entry.
    crate::sync::try_lock(&LIFECYCLE)
}

fn host_from<'a>(user: *mut c_void) -> &'a Host {
    unsafe { &*user.cast::<Host>() }
}

fn empty_plan() -> Config {
    Config::plan(false, 0, [0, 0], [0, 0])
}

impl Host {
    fn user(&self) -> Handle {
        ptr::from_ref(self).cast_mut().cast()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn log_bytes(&self, message: &[u8]) {
        let mut file = lock(&self.log_file);
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
        unsafe { (self.set_engine_spatial)(u32::from(backend == RSF_SR_SPATIAL), quality) != 0 }
    }

    fn request_reconfigure(&self, state: &mut State, backend: u32, quality: u32) -> bool {
        if let Some(adapter) = &self.policy_v2 {
            let mut generation = 0;
            if unsafe {
                (adapter.request)(
                    u32::from(backend == RSF_SR_SPATIAL),
                    quality,
                    &mut generation,
                )
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

    // The saved settings with `start` for keys never saved, and the stored generation mode.
    fn read_settings(&self, start: &Settings) -> (Settings, u32) {
        let defaults = sharpening_policy::defaults();
        unsafe { read_settings_and_fg_mode(self.preferences.as_ptr(), SECTION, start, &defaults) }
            .unwrap_or((*start, start.fg_enabled))
    }

    fn save(&self, s: &State) {
        if unsafe { rsf_settings_store_write(self.preferences.as_ptr(), SECTION, &s.settings) }
            != OK
        {
            self.log("Could not save the settings; the change applies for this session.");
        }
    }

    // The saved generation options over the ones the adapter installed generation with.
    fn restore_generation(&self, s: &mut State, fg: &mut FgOptions) {
        let started = Settings {
            fg_enabled: fg.mode,
            generated_frames: fg.generated_frames,
            latency: fg.reflex_mode,
            frame_limit_us: fg.frame_limit_us,
            ..s.settings
        };
        let (saved, mode) = self.read_settings(&started);
        let restored = FgOptions {
            mode,
            generated_frames: saved.generated_frames,
            reflex_mode: saved.latency,
            frame_limit_us: saved.frame_limit_us,
            ..*fg
        };
        let generation =
            |o: &FgOptions| (o.mode, o.generated_frames, o.reflex_mode, o.frame_limit_us);
        (s.settings.fg_enabled, s.settings.generated_frames) = (mode, saved.generated_frames);
        (s.settings.latency, s.settings.frame_limit_us) = (saved.latency, saved.frame_limit_us);
        if generation(&restored) != generation(fg) {
            *fg = restored;
            unsafe { leaf::fg_options(fg) };
        }
    }

    fn destroy(&self, context: &mut Handle) {
        if !context.is_null() {
            unsafe { leaf::sr12_destroy(*context) };
            *context = null_mut();
        }
    }

    fn release_rendering(&self, stop_overlay: bool) {
        if stop_overlay {
            leaf::overlay_stop();
        }
        let mut state = self.lock();
        self.release_views(&mut state);
        self.destroy(&mut state.inputs);
    }

    // At a drained point only: reconfiguration, new output extents or stop.
    fn release_views(&self, s: &mut State) {
        for mut view in std::mem::take(&mut s.views) {
            self.destroy(&mut view.sr);
        }
    }

    // Views other than `current` unseen for IDLE_SOURCE_FRAMES whose last recording is retired.
    fn reap(&self, s: &mut State, current: u64, source: u64) {
        let slots = s.slot_serials;
        let idle = |view: &View| {
            view.key != current
                && source.saturating_sub(view.seen) >= IDLE_SOURCE_FRAMES
                && (view.serial == 0 || slots[view.slot as usize] > view.serial)
        };
        for mut view in s.views.extract_if(.., |view| idle(view)) {
            self.destroy(&mut view.sr);
        }
    }

    fn set_sharpening(&self, s: &State) {
        for view in &s.views {
            let _ = unsafe { leaf::sr12_set_sharpening(view.sr, &s.sharpening) };
        }
    }

    // The vendor context setup for the planned backend and output.
    fn vendor_setup(&self, s: &State, device: Handle, inverted: u32) -> SrSetup {
        let mut setup = self.vendors.sr_setup(
            device,
            (s.backend, s.quality, s.preset),
            Some(log),
            self.user(),
        );
        // The host plans the render extents; the vendor context reports them.
        (setup.output_width, setup.output_height) = (s.width, s.height);
        setup.inverted_depth = inverted;
        setup.streamline_host = leaf::present_host();
        setup
    }

    fn render(&self, commands: Handle, pass: &Pass, begin: bool) {
        if unsafe { leaf::depth_domains(pass) } != 0 {
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
                unsafe { leaf::fg_submitted(pass.session_id, pass.source_frame_id) };
            }
            return;
        }
        match pass.role {
            RSF_GAME_RENDER_FINAL_SCENE => unsafe { leaf::fg_hudless(commands, pass) },
            RSF_GAME_RENDER_WINDOW => {
                // The adapter delivers a drained change here too, so a switch never waits for a
                // scene pass the engine only records under the old configuration.
                if pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0 {
                    self.reconfigure(&mut self.lock(), commands, pass);
                }
                self.window(commands, pass)
            }
            RSF_GAME_RENDER_SR | RSF_GAME_RENDER_FG_INPUTS => {
                let mut state = self.lock();
                self.scene(&mut state, commands, pass);
            }
            _ => {}
        }
    }

    // Window pass: provider switch bookkeeping, the shared panel and its requested changes.
    fn window(&self, queue: Handle, pass: &Pass) {
        let mut guard = self.lock();
        let s = &mut *guard;
        unsafe { leaf::fg_window(pass) };
        let Some(device) = (unsafe { com_ref::owned(leaf::child_device(queue)) }) else {
            return;
        };
        if s.save_pending {
            if leaf::present_switch_result() != 0 {
                s.save_pending = false;
                self.log("Generation provider switch failed; the saved choice is unchanged.");
            } else if leaf::present_backend() == s.save_target {
                s.save_pending = false;
                let _ = leaf::fg_choice_save(s.save_backend);
                // Off is fg_choice's own value; the settings keep the last provider.
                if s.save_backend != 0 {
                    s.settings.fg_backend = s.save_backend;
                    self.save(s);
                }
            }
        }
        let mut adapter = Adapter::default();
        let vendor = (unsafe { leaf::adapter(device.as_raw(), &mut adapter) } != 0)
            .then_some(adapter.vendor);
        s.vendor = vendor.unwrap_or(0);
        let mut stats = Stats {
            struct_size: size_of::<Stats>() as u32,
            backend_loaded: 1,
            backend_supported: s.plan.enabled,
            sr_change_id: s.sr_change_id,
            sr_change_status: s.sr_change_status,
            requested_backend: s.requested_backend,
            quality: s.quality,
            sharpening_available: u32::from((RSF_SR_DLSS..=RSF_SR_XESS).contains(&s.backend)),
            dlss_preset: s.preset,
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
            jitter_active: u32::from(s.plan.enabled != 0 && s.backend != RSF_SR_SPATIAL),
            render_scale_percent: s
                .plan
                .render_width
                .wrapping_mul(100)
                .checked_div(s.width)
                .unwrap_or(0),
            ..Stats::default()
        };
        // Auto stays the selected choice, named by the provider it resolved to. FSR1 here is the
        // engine's own scaler, which the adapter reports (`set_engine_spatial`).
        let selected = if s.automatic { RSF_SR_AUTO } else { s.backend };
        let offered = self.vendors.offered(vendor, self.sr_choices);
        sr_card(&mut stats, selected, s.backend, offered);
        if let Some(adapter) = &self.policy_v2
            && s.backend == RSF_SR_SPATIAL
            && !adapter.spatial_name.is_empty()
        {
            stats.backend_name = adapter.spatial_name.as_ptr();
        }
        fill_sharpening_stats(&mut stats, &s.sharpening);
        let reason: Option<&'static CStr> = if s.dirty {
            Some(c"Applying requested configuration after GPU completion")
        } else if s.last_result != 0 {
            Some(c"Provider refused; spatial fallback retained")
        } else if s.accepted == 0 && s.backend != RSF_SR_SPATIAL {
            Some(c"Prepared; awaiting an eligible scene evaluation")
        } else if s.backend == RSF_SR_SPATIAL {
            Some(c"Sharpening unavailable with engine spatial upscaling")
        } else {
            None
        };
        stats.refusal_reason = reason.map_or(null(), CStr::as_ptr);
        s.overlay_frames += 1;
        stats.frames_presented = s.overlay_frames as u32;
        let mut presented = 0u32;
        if unsafe { leaf::last_present_count(pass.swapchain, &mut presented) } != 0 {
            stats.application_presented_frames = u64::from(presented);
        }
        stats.sample_qpc = crate::now_qpc();
        stats.qpc_frequency = crate::qpc_frequency();
        let mut fg = FgStatus {
            struct_size: size_of::<FgStatus>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let have_fg = unsafe { leaf::fg_status(&mut fg) } != 0;
        if have_fg && !s.generation_restored {
            s.generation_restored = true;
            self.restore_generation(s, &mut fg.requested);
        }
        unsafe { leaf::overlay_fill_fg(&mut stats, if have_fg { &fg } else { null() }) };
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
            leaf::overlay_frame(
                device.as_raw(),
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
        if intent.fg_backend_changed != 0 {
            // Auto is the provider this GPU prefers among those the bridge serves.
            let target = fg_choice::resolve(
                intent.fg_backend,
                fg_choice::rsf_fg_default_backend(s.vendor, 0),
                crate::d3d11_present::rsf_d3d11_present_backend_choices(),
            );
            let applied = if target == 0 && intent.fg_backend != 0 {
                BACKEND_OK
            } else {
                leaf::present_request(target)
            };
            if applied == BACKEND_OK {
                s.save_pending = true;
                s.save_backend = intent.fg_backend;
                s.save_target = if target == 0 && intent.fg_backend != 0 {
                    leaf::present_backend()
                } else {
                    target
                };
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
                fg.frame_limit_us = intent.frame_limit_us;
            }
            unsafe { leaf::fg_options(fg) };
            (s.settings.fg_enabled, s.settings.generated_frames) = (fg.mode, fg.generated_frames);
            (s.settings.latency, s.settings.frame_limit_us) = (fg.reflex_mode, fg.frame_limit_us);
        }
        if intent.performance_hud_changed != 0 {
            s.show_performance = intent.performance_hud != 0;
            s.settings.status_overlay = u32::from(s.show_performance);
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
            if sharpening_policy::valid(&requested) {
                s.sharpening = requested;
                set_sharpening(&mut s.settings, &requested);
                self.set_sharpening(s);
            } else {
                self.log("Could not save sharpening settings; previous values retained.");
            }
        }
        self.apply_policy(s, intent);
        // One section write of everything accepted, as the other hosts save their settings.
        let persisted = intent.fg_changed != 0
            || intent.reflex_changed != 0
            || intent.frame_limit_changed != 0
            || intent.performance_hud_changed != 0
            || intent.sharpening_changed != 0
            || intent.dlss_preset_changed != 0
            || intent.backend_changed != 0
            || intent.quality_changed != 0
            || intent.enabled_changed != 0;
        if persisted {
            self.save(s);
        }
    }

    // A backend, quality, enable or DLSS model change the adapter's policy accepts.
    fn apply_policy(&self, s: &mut State, intent: &Intent) {
        let preset_changed = intent.dlss_preset_changed != 0
            && intent.dlss_preset != s.preset
            && leaf::dlss_preset_valid(intent.dlss_preset) != 0;
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
            s.automatic = backend == RSF_SR_AUTO;
        }
        if intent.enabled_changed != 0 {
            s.enabled = intent.enabled != 0;
        }
        s.settings.sr_backend = if s.automatic { RSF_SR_AUTO } else { backend };
        (s.settings.sr_quality, s.settings.sr_enabled) = (quality, u32::from(s.enabled));
        if preset_changed {
            s.preset = intent.dlss_preset;
            s.settings.dlss_preset = s.preset;
            self.log("DLSS model change requested; draining previous GPU work.");
        } else {
            self.log("Overlay requested vendor/quality change; draining previous GPU work.");
        }
    }

    // Applies a requested change on the pass that proves the earlier GPU work retired, and new
    // output extents, then prepares the first pass after either. False ends this pass.
    fn reconfigure(&self, s: &mut State, commands: Handle, pass: &Pass) -> bool {
        let camera = &pass.camera;
        let mut drained = pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0;
        if let (Some(adapter), Some(expected)) = (&self.policy_v2, s.requested_generation) {
            let mut generation = 0;
            drained = drained
                && unsafe { (adapter.drained)(&mut generation) } != 0
                && generation == expected;
            if !drained {
                return false;
            }
        }
        if (s.dirty && drained)
            || s.width != camera.output_width
            || s.height != camera.output_height
        {
            // The adapter's submission path joins its previous frame-fence values before this.
            self.release_views(s);
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
            return prepared;
        }
        true
    }

    // SR and generation-input passes on the adapter's recording thread.
    fn scene(&self, s: &mut State, commands: Handle, pass: &Pass) {
        let camera = &pass.camera;
        if !self.reconfigure(s, commands, pass) {
            return;
        }
        if pass.role == RSF_GAME_RENDER_FG_INPUTS && pass.camera_valid != 0 {
            if s.inputs.is_null()
                || s.input_width != camera.render_width
                || s.input_height != camera.render_height
            {
                self.destroy(&mut s.inputs);
                let Some(device) = (unsafe { com_ref::owned(leaf::child_device(commands)) }) else {
                    return;
                };
                let setup = SrSetup {
                    struct_size: size_of::<SrSetup>() as u32,
                    abi_version: 1,
                    device: device.as_raw(),
                    output_width: camera.output_width,
                    output_height: camera.output_height,
                    render_width: camera.render_width,
                    render_height: camera.render_height,
                    log: Some(log),
                    user: self.user(),
                    ..SrSetup::default()
                };
                if unsafe { leaf::sr12_create(&setup, &mut s.inputs) } != BACKEND_OK {
                    return;
                }
                s.input_width = setup.render_width;
                s.input_height = setup.render_height;
            }
            if let Some(slot) = self.record(s, commands) {
                let _ = unsafe { leaf::sr12_evaluate(s.inputs, commands, pass, slot) };
            }
            return;
        }
        // The engine's scaler, native rendering and a refused plan have no vendor context.
        if s.views.is_empty() || pass.camera_valid == 0 {
            return;
        }
        // This recording proves its slot's previous one retired, before any view is reaped.
        let Some(slot) = self.record(s, commands) else {
            return;
        };
        self.reap(s, pass.view_key, pass.source_frame_id);
        let index = match s.views.iter().position(|view| view.key == pass.view_key) {
            Some(index) => index,
            None => match self.create_view(s, commands, pass) {
                Ok(index) => index,
                Err(result) => {
                    // Kept without a context, so the view is refused once until it is reaped.
                    self.log(&format!(
                        "SR context for view {} refused: {result}",
                        pass.view_key
                    ));
                    s.views.push(View {
                        key: pass.view_key,
                        sr: null_mut(),
                        seen: pass.source_frame_id,
                        slot: 0,
                        serial: 0,
                    });
                    return;
                }
            },
        };
        if s.views[index].sr.is_null() {
            s.views[index].seen = pass.source_frame_id;
            return;
        }
        let now = unsafe { leaf::tick() };
        if now >= s.sharpening_poll {
            s.sharpening_poll = now.saturating_add(SHARPENING_POLL_MS);
            s.sharpening = self.read_sharpening();
            self.set_sharpening(s);
        }
        let view = &mut s.views[index];
        (view.seen, view.slot, view.serial) = (pass.source_frame_id, slot, s.serial);
        let result = unsafe { leaf::sr12_evaluate(view.sr, commands, pass, slot) };
        if result != BACKEND_OK && s.refused == 0 {
            let resources = [pass.color_input, pass.depth, pass.motion, pass.color_output];
            for (index, &resource) in resources.iter().enumerate() {
                if resource.is_null() {
                    continue;
                }
                let mut desc = ResourceDesc::default();
                unsafe { leaf::resource_desc(resource, &mut desc) };
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

    // A recording on the slot bound to `commands`, which proves that slot's previous one retired.
    fn record(&self, s: &mut State, commands: Handle) -> Option<u32> {
        let slot = s.command_lists.bind(commands)?;
        s.serial += 1;
        s.slot_serials[slot as usize] = s.serial;
        Some(slot)
    }

    // The pass's view's vendor context for the planned backend, or the creation's refusal.
    fn create_view(&self, s: &mut State, commands: Handle, pass: &Pass) -> Result<usize, i32> {
        let Some(device) = (unsafe { com_ref::owned(leaf::child_device(commands)) }) else {
            return Err(BACKEND_NOT_SUPPORTED);
        };
        let setup = self.vendor_setup(s, device.as_raw(), pass.camera.depth_inverted);
        let mut sr = null_mut();
        let created = unsafe { leaf::sr12_create(&setup, &mut sr) };
        if created != BACKEND_OK {
            return Err(created);
        }
        let _ = unsafe { leaf::sr12_set_sharpening(sr, &s.sharpening) };
        s.views.push(View {
            key: pass.view_key,
            sr,
            seen: pass.source_frame_id,
            slot: 0,
            serial: 0,
        });
        Ok(s.views.len() - 1)
    }

    // The first SR pass after start or reconfiguration: resolve Auto, then plan the engine's
    // spatial path or prepare the vendor context. False ends this pass.
    fn prepare(&self, s: &mut State, commands: Handle, pass: &Pass) -> bool {
        s.attempted = true;
        s.plan = Config::plan(false, 0, [s.width, s.height], [0, 0]);
        if s.automatic {
            let mut adapter = Adapter::default();
            let identified = unsafe { com_ref::owned(leaf::child_device(commands)) }
                .is_some_and(|device| unsafe { leaf::adapter(device.as_raw(), &mut adapter) } != 0);
            if !identified {
                s.last_result = BACKEND_NOT_SUPPORTED;
                self.log("Automatic SR refused: owning D3D12 adapter could not be identified.");
                return false;
            }
            // Among the providers this machine serves, as the panel offers them.
            let offered = self.vendors.offered(Some(adapter.vendor), self.sr_choices);
            s.backend = crate::rsf_gpu_select_sr_backend(adapter.vendor, adapter.software, offered);
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
        if s.backend == RSF_SR_NONE || !s.enabled {
            // Native rendering still supplies generation inputs.
            return true;
        }
        if s.backend == RSF_SR_SPATIAL {
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
        if unsafe { com_ref::owned(leaf::child_device(commands)) }.is_none() {
            return false;
        }
        let now = unsafe { leaf::tick() };
        if now >= s.sharpening_poll {
            s.sharpening = self.read_sharpening();
            s.sharpening_poll = now.saturating_add(SHARPENING_POLL_MS);
        }
        let created = match self.create_view(s, commands, pass) {
            Ok(index) => {
                let sr = s.views[index].sr;
                let _ = unsafe {
                    leaf::sr12_plan(sr, &mut s.plan.render_width, &mut s.plan.render_height)
                };
                s.plan.enabled = 1;
                BACKEND_OK
            }
            Err(result) => result,
        };
        s.last_result = created;
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
    if !unsafe { has_capacity(output) } {
        return 0;
    }
    let (plan, backend) = {
        let s = host_from(owner).lock();
        (s.plan, s.backend)
    };
    // The plan with its backend's vendor sampling for the planned ratio.
    let enabled = plan.enabled != 0;
    let output_extent = [plan.output_width, plan.output_height];
    let render_extent = [plan.render_width, plan.render_height];
    let backend = if enabled { backend } else { 0 };
    unsafe { output.write(Config::plan(enabled, backend, output_extent, render_extent)) };
    1
}

unsafe extern "C" fn render(owner: *mut c_void, commands: Handle, pass: *const Pass, begin: u32) {
    if commands.is_null() {
        return;
    }
    if let Some(pass) = unsafe { checked(pass) } {
        host_from(owner).render(commands, pass, begin != 0);
    }
}

unsafe extern "C" fn cpu_event(_: *mut c_void, event: *const CpuEvent) {
    unsafe { leaf::fg_cpu(event) };
}

// The services an attaching adapter receives, from a full V3 setup.
unsafe fn attached(setup: *const Setup) -> Option<*mut Services> {
    let size = unsafe { setup.cast::<u32>().read() } as usize;
    (size >= size_of::<SetupV3>())
        .then(|| unsafe { (*setup.cast::<SetupV3>()).services })
        .filter(|services| !services.is_null())
}

// Admission and host construction, before any plugin is loaded. Err is the public refusal code:
// 1 incomplete or mismatched setup, 2 invalid choice, 4 log unavailable, 8 adapter policy refused.
unsafe fn describe(setup: *const Setup) -> Result<Host, u32> {
    let setup_ptr = setup;
    let Some(setup) = (unsafe { checked(setup) }) else {
        return Err(1);
    };
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
                spatial_name: unsafe { copy_c_string(extended.spatial_name_utf8, c"") },
            })
        }
        RSF_CALLBACK_HOST_ABI_VERSION_V3 if unsafe { attached(setup_ptr) }.is_some() => None,
        _ => return Err(1),
    };
    // An attaching adapter owns its plugin lifecycle; a started one names the plugin to load.
    let plugin = setup.abi_version != RSF_CALLBACK_HOST_ABI_VERSION_V3;
    if setup.plugin_path.is_null() == plugin
        || setup.probe.is_null() == plugin
        || setup.log_path.is_null()
        || setup.preferences_path.is_null()
        || setup.engine_version_utf8.is_null()
        || setup.project_id_utf8.is_null()
        || setup.sr_backend_choices == 0
    {
        return Err(1);
    }
    let automatic = setup.backend == RSF_SR_AUTO;
    let backend = if automatic {
        RSF_SR_NONE
    } else {
        setup.backend
    };
    let quality = setup.quality;
    if leaf::dlss_preset_valid(setup.dlss_preset) == 0 || !accepts_backend_quality(backend, quality)
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
    // FSR2, FSR3 and FSR4 share one FidelityFX package; every adapter names it in each field.
    let paths = unsafe {
        VendorPaths::from_folders(
            copy_c_string(setup.streamline_directory_utf8, c""),
            copy_c_string(setup.fsr3_directory_utf8, c""),
            copy_c_string(setup.xess_directory_utf8, c""),
        )
    };
    let preferences = unsafe { wide_copy(setup.preferences_path) };
    let ui_preferences = unsafe { rsf_settings_store_read_layout(preferences.as_ptr(), SECTION) };
    let host = Host {
        state: Mutex::new(State {
            views: Vec::new(),
            inputs: null_mut(),
            slot_serials: [0; 3],
            serial: 0,
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
            settings: unsafe { std::mem::zeroed() },
            generation_restored: false,
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
            save_target: 0,
            vendor: 0,
        }),
        log_file: Mutex::new(log_file),
        plugin: AtomicPtr::new(null_mut()),
        preferences,
        sr_choices: setup.sr_backend_choices,
        vendors: Vendors {
            paths,
            engine_type: setup.engine_type,
            application_id: 0,
            engine_version: unsafe { copy_c_string(setup.engine_version_utf8, c"") },
            project_id: unsafe { copy_c_string(setup.project_id_utf8, c"") },
        },
        set_engine_spatial,
        policy_v2,
    };
    // The saved settings over the adapter's start values: its SR choice, and the options it
    // installed generation with when generation already reports them.
    let mut fg = FgStatus {
        struct_size: size_of::<FgStatus>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let generation = if unsafe { leaf::fg_status(&mut fg) } != 0 {
        fg.requested
    } else {
        FgOptions {
            mode: RSF_FG_FIXED,
            generated_frames: 1,
            reflex_mode: RSF_REFLEX_OFF,
            ..unsafe { std::mem::zeroed() }
        }
    };
    let start = Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        sr_enabled: 1,
        sr_backend: setup.backend,
        sr_quality: quality,
        dlss_preset: setup.dlss_preset,
        fg_enabled: generation.mode,
        fg_backend: RSF_FG_BACKEND_AUTO,
        generated_frames: generation.generated_frames,
        latency: generation.reflex_mode,
        frame_limit_us: generation.frame_limit_us,
        ..unsafe { std::mem::zeroed() }
    };
    let (mut settings, fg_mode) = host.read_settings(&start);
    settings.fg_enabled = fg_mode;
    let saved = if settings.sr_backend == RSF_SR_AUTO {
        RSF_SR_NONE
    } else {
        settings.sr_backend
    };
    if !accepts_backend_quality(saved, settings.sr_quality)
        || leaf::dlss_preset_valid(settings.dlss_preset) == 0
    {
        (settings.sr_backend, settings.sr_quality) = (setup.backend, quality);
        settings.dlss_preset = setup.dlss_preset;
    }
    let sharpening = host.read_sharpening();
    set_sharpening(&mut settings, &sharpening);
    let (backend, quality) = {
        let mut s = host.lock();
        s.automatic = settings.sr_backend == RSF_SR_AUTO;
        s.backend = if s.automatic {
            RSF_SR_NONE
        } else {
            settings.sr_backend
        };
        (s.requested_backend, s.quality) = (s.backend, settings.sr_quality);
        s.requested_quality = s.quality;
        s.preset = settings.dlss_preset;
        s.enabled = settings.sr_enabled != 0;
        s.show_performance = settings.status_overlay != 0;
        s.sharpening = sharpening;
        s.settings = settings;
        (s.backend, s.quality)
    };
    if !host.policy(backend, quality) {
        return Err(8);
    }
    Ok(host)
}

fn retire_plugin(plugin: *mut Session) -> bool {
    plugin.is_null()
        || (unsafe { leaf::session_quiesce(plugin) } == OK
            && unsafe { leaf::session_stop(plugin) } == OK)
}

// Refusal codes beyond describe: 9 a refused plugin could not be stopped and the host stays
// published to retry, 10 the plugin refused preparation or activation, 11 no host allocation.
unsafe fn start(setup: *const Setup) -> u32 {
    let Some(mut published) = try_lifecycle() else {
        return 1;
    };
    if !published.0.is_null() {
        return 1;
    }
    let host = match unsafe { describe(setup) } {
        Ok(host) => host,
        Err(code) => return code,
    };
    let attach = unsafe { attached(setup) };
    let setup = unsafe { &*setup };
    // The plugin may keep the services' user address until its session stops.
    let raw = crate::allocate_handle(host);
    if raw.is_null() {
        return 11;
    }
    let host = unsafe { &*raw };
    let services = Services {
        struct_size: size_of::<Services>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        session_id: unsafe { leaf::tick() }.wrapping_add(1),
        log: Some(log),
        render_pass: Some(render),
        render_config: Some(configuration),
        user: raw.cast(),
        cpu_event: Some(cpu_event),
        ..unsafe { std::mem::zeroed() }
    };
    if let Some(output) = attach {
        unsafe { output.write(services) };
        host.log("Adapter attached; awaiting actual D3D12 render callback.");
        published.0 = raw;
        return 0;
    }
    let options = SessionOptions {
        struct_size: size_of::<SessionOptions>() as u32,
        abi_version: SESSION_ABI_VERSION,
        plugin_path: setup.plugin_path,
        probe: setup.probe,
        services,
    };
    let mut plugin = null_mut();
    let mut result = unsafe { leaf::session_prepare(&options, &mut plugin) };
    if result == OK {
        result = unsafe { leaf::session_start(plugin) };
    }
    host.plugin.store(plugin, Ordering::Release);
    if result != OK {
        host.log("Native renderer plugin preparation/activation refused.");
        if !retire_plugin(plugin) {
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
    let plugin = host.plugin.load(Ordering::Acquire);
    if !retire_plugin(plugin) {
        return 1;
    }
    host.plugin.store(null_mut(), Ordering::Release);
    host.release_rendering(true);
    leaf::input_dump_cancel(0);
    host.log("Native renderer SR stopped after managed/native/GPU retirement.");
    published.0 = null_mut();
    drop(unsafe { Box::from_raw(raw) });
    0
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_callback_host_start(setup: *const Setup) -> u32 {
    unsafe { start(setup) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_callback_host_stop() -> u32 {
    stop()
}

// The published host for a status or settings call. Start and stop hold it meanwhile.
fn with_host<T>(call: impl FnOnce(&Host) -> T) -> Option<T> {
    let published = try_lifecycle()?;
    (!published.0.is_null()).then(|| call(unsafe { &*published.0 }))
}

/// # Safety
/// `output` is null or readable for its leading `struct_size`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_callback_host_status_get(output: *mut HostStatus) -> u32 {
    if !unsafe { has_capacity(output) } {
        return 0;
    }
    with_host(|host| {
        let s = host.lock();
        let planned = s.plan.enabled != 0;
        let status = HostStatus {
            struct_size: size_of::<HostStatus>() as u32,
            requested_backend: if s.automatic {
                RSF_SR_AUTO
            } else {
                s.requested_backend
            },
            active_backend: if planned { s.backend } else { RSF_SR_NONE },
            enabled: s.plan.enabled,
            result: s.last_result,
            render_width: s.plan.render_width,
            render_height: s.plan.render_height,
            output_width: s.width,
            output_height: s.height,
            evaluated_frames: s.accepted,
            refused_frames: s.refused,
        };
        unsafe { output.write(status) };
        1
    })
    .unwrap_or(0)
}

// rsf_render_settings_get and _apply for an attached or started host; None without one.
pub(super) fn settings_get() -> Option<Settings> {
    with_host(|host| host.lock().settings)
}

pub(super) fn settings_apply(request: &Settings) -> Option<i32> {
    with_host(|host| host.settings_apply(request))
}

// Whether a request without a host would pass any host's checks, so an adapter can refuse it
// before the host it will be applied by exists.
pub(super) fn settings_acceptable(request: &Settings) -> bool {
    refused(request, rsf_game_sys::RSF_SR_CHOICES_ALL) == 0
}

// A settings request as the panel change it amounts to against what is saved.
fn settings_intent(saved: &Settings, request: &Settings) -> Intent {
    let changed = |before: u32, after: u32| u32::from(before != after);
    Intent {
        struct_size: size_of::<Intent>() as u32,
        backend_changed: changed(saved.sr_backend, request.sr_backend),
        backend: request.sr_backend,
        quality_changed: changed(saved.sr_quality, request.sr_quality),
        quality: request.sr_quality,
        enabled_changed: changed(saved.sr_enabled, request.sr_enabled),
        enabled: request.sr_enabled,
        dlss_preset_changed: changed(saved.dlss_preset, request.dlss_preset),
        dlss_preset: request.dlss_preset,
        fg_backend_changed: changed(saved.fg_backend, request.fg_backend),
        fg_backend: request.fg_backend,
        fg_changed: u32::from(
            saved.fg_enabled != request.fg_enabled
                || saved.generated_frames != request.generated_frames,
        ),
        fg_mode: request.fg_enabled,
        fg_generated: request.generated_frames,
        reflex_changed: changed(saved.latency, request.latency),
        reflex_mode: request.latency,
        frame_limit_changed: changed(saved.frame_limit_us, request.frame_limit_us),
        frame_limit_us: request.frame_limit_us,
        performance_hud_changed: changed(saved.status_overlay, request.status_overlay),
        performance_hud: request.status_overlay,
        sharpening_changed: 1,
        sharpening_mode: request.sharpening_mode,
        sharpening_sharpness: request.sharpening_sharpness,
        sharpening_mas: request.sharpening_mas,
        sharpening_motion_sharpness: request.sharpening_motion_sharpness,
        sharpening_motion_threshold: request.sharpening_motion_threshold,
        sharpening_motion_scale_limit: request.sharpening_motion_scale_limit,
        sharpening_contrast_enabled: request.sharpening_contrast_enabled,
        sharpening_contrast: request.sharpening_contrast,
        sharpening_depth_scale: request.sharpening_depth_scale,
        sharpening_depth_bias: request.sharpening_depth_bias,
        sharpening_clamp_output: request.sharpening_clamp_output,
        sharpening_debug: request.sharpening_debug,
        ..Intent::default()
    }
}

impl Host {
    // A request from the adapter's own settings API: checked by the Runtime's settings rule, then
    // applied and saved exactly as the same change made in the panel. FG mode is fg_enabled's
    // stored value (0 off, 1 fixed, 2 auto, 3 dynamic).
    fn settings_apply(&self, request: &Settings) -> i32 {
        if refused(request, self.sr_choices) != 0 {
            return -1;
        }
        let mut fg = FgStatus {
            struct_size: size_of::<FgStatus>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let mut s = self.lock();
        if unsafe { leaf::fg_status(&mut fg) } == 0 {
            // No generation yet: the saved options stand for the ones it will report.
            fg.requested = FgOptions {
                struct_size: size_of::<FgOptions>() as u32,
                mode: s.settings.fg_enabled,
                generated_frames: s.settings.generated_frames,
                reflex_mode: s.settings.latency,
                frame_limit_us: s.settings.frame_limit_us,
                debug: 0,
            };
        }
        let intent = settings_intent(&s.settings, request);
        self.apply_intent(&mut s, &intent, &mut fg.requested);
        OK
    }
}
