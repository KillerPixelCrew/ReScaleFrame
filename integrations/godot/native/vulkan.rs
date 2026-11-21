// SPDX-License-Identifier: MIT
//! Godot Vulkan session state formerly held by `vulkan.cpp`: the boot state and session identity,
//! the requested settings and their generation, the status record (ABI 3) with its latency
//! overlay, the configure admission, the presenter currency rule for acquire, the FG enable rule,
//! the fixed-guide retirement and the SR and FG accounting.
//!
//! `vulkan.cpp` keeps its guard and every Vulkan, Streamline, XeSS, FidelityFX, presenter,
//! capture and SR context call. Each transition that ran under that guard still does: boot,
//! shutdown and configure run inside `Platform::vulkan_locked`, and the render, presentation and
//! capture leaves call the `rsf_godot_vk_*` exports while they hold it. The lock order is the
//! guard, then `state`; no leaf runs while `state` is held.
//!
//! The Vulkan admission stays distinct from the D3D12 one: the shared `rsfg_configure` refusal
//! rules in `control.rs` run first, then these device, provider and presenter checks, and the
//! latency options are applied before the request is stored, as before.
use crate::control::default_settings;
use crate::platform::{
    ABI_VERSION, Platform, Settings, Setup, Status, VkFacts, VkFgPass, VkLatencyFacts,
    VkPresentFacts, VkSrFrame, VkView,
};
use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const BACKEND_OK: i32 = 0;
const BACKEND_ERROR_NOT_SUPPORTED: i32 = -7;
const BACKEND_ERROR_NOT_READY: i32 = -8;
const SR_FSR3: u32 = 3;
const SR_FSR4: u32 = 4;
const SR_XESS: u32 = 5;
const SR_AUTO: u32 = 7;
const FG_DLSS: u32 = 1;
const FG_FSR3: u32 = 3;
const FG_FSR4: u32 = 4;
const FG_XESS: u32 = 5;
const FG_BACKEND_AUTO: u32 = 7;
const SCREEN_FLIGHT: u32 = 4;
const RENDER_SR: u32 = 1;
/// The first registered window message; the PCL ping message is one when Streamline bound it.
const REGISTERED_MESSAGE: u32 = 0xc000;
/// `RSF_GODOT_VK_SR_*`: how an SR pass was counted.
pub(crate) const SR_EVALUATED: u32 = 1;
pub(crate) const SR_REFUSED: u32 = 2;

/// Vulkan resolves an Auto SR request to FSR3.
fn sr_backend(settings: &Settings) -> u32 {
    if settings.sr_backend == SR_AUTO {
        SR_FSR3
    } else {
        settings.sr_backend
    }
}

/// Vulkan resolves an Auto FG provider to FSR3.
fn fg_backend(settings: &Settings) -> u32 {
    if settings.fg_backend == FG_BACKEND_AUTO {
        FG_FSR3
    } else {
        settings.fg_backend
    }
}

/// FSR4 and XeSS FG presenters allocate their guides for one SR plan.
fn fixed_guides(backend: u32) -> bool {
    backend == FG_FSR4 || backend == FG_XESS
}

/// The identity of the first SR pass a fixed-guide presenter recorded.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Guides {
    session: u64,
    view: u32,
    width: u32,
    height: u32,
}

struct State {
    settings: Settings,
    generation: u64,
    status: Status,
    /// The generation the live presenter was created for; zero retires it at the next acquire.
    presenter_generation: u64,
    guides: Option<Guides>,
}

fn initial_status() -> Status {
    Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: ABI_VERSION,
        ..Status::default()
    }
}

pub(crate) struct Vulkan {
    active: AtomicBool,
    state: Mutex<State>,
}

impl Vulkan {
    pub(crate) fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            state: Mutex::new(State {
                settings: default_settings(),
                generation: 1,
                status: initial_status(),
                presenter_generation: 0,
                guides: None,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn booted(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// `setup` passed the size and ABI checks; its strings are borrowed for the call.
    pub(crate) fn boot(&self, platform: &Platform, setup: &Setup) -> c_int {
        if setup.runtime_directory.is_null() || !platform.vulkan_supported() {
            return 0;
        }
        platform.vulkan_locked(|| {
            if self.booted() || platform.vulkan_facts().sdk_shutdown_pending != 0 {
                return 0;
            }
            {
                // The generation keeps counting across sessions, as before.
                let mut state = self.state();
                state.settings = default_settings();
                state.status = initial_status();
                state.status.session_id = platform.tick_count().wrapping_add(1);
                state.status.fg_result = BACKEND_ERROR_NOT_SUPPORTED;
            }
            if platform.vulkan_start(setup) == 0 {
                return 0;
            }
            self.active.store(true, Ordering::Release);
            1
        })
    }

    /// The caller found the session booted. A refused stop keeps it booted.
    pub(crate) fn shutdown(&self, platform: &Platform) -> c_int {
        platform.vulkan_locked(|| {
            if platform.vulkan_stop() == 0 {
                return 0;
            }
            self.active.store(false, Ordering::Release);
            let session = {
                let mut state = self.state();
                state.status.active_sr = 0;
                state.status.sr_result = BACKEND_ERROR_NOT_READY;
                state.status.session_id
            };
            platform.vulkan_release(session);
            1
        })
    }

    /// `settings` passed the shared `rsfg_configure` refusal rules.
    pub(crate) fn configure(&self, platform: &Platform, settings: &Settings) -> c_int {
        if !self.booted() {
            return 0;
        }
        platform.vulkan_locked(|| {
            if !self.booted() || !self.admit(platform, settings) {
                return 0;
            }
            if platform.vulkan_latency_options(settings.latency_mode, settings.frame_limit_us)
                != BACKEND_OK
            {
                return 0;
            }
            let mut state = self.state();
            let current = &state.settings;
            if current.sr_backend != settings.sr_backend
                || current.quality != settings.quality
                || current.dlss_preset != settings.dlss_preset
            {
                state.generation = state.generation.wrapping_add(1);
            }
            state.settings = *settings;
            1
        })
    }

    /// Device readiness and provider refusals, in the order `vulkan.cpp` applied them. Runs under
    /// the guard; the FSR4 FG probe and the presenter capacity are asked only when reached.
    fn admit(&self, platform: &Platform, value: &Settings) -> bool {
        let facts: VkFacts = platform.vulkan_facts();
        let on = |flag: u32| flag != 0;
        if value.sr_backend == SR_XESS && !on(facts.xess_device_ready) {
            return false;
        }
        // These paths are added independently. Do not accept unavailable FG or silently
        // substitute an FSR3 provider for a requested DLSS, XeSS or FSR4 path.
        if value.sr_backend == SR_FSR4
            && !(on(facts.fsr4_sr_compiled)
                && on(facts.graph_handoff)
                && on(facts.interop_device_ready))
        {
            return false;
        }
        let selected = fg_backend(value);
        if !matches!(selected, FG_DLSS | FG_FSR3 | FG_FSR4 | FG_XESS) {
            return false;
        }
        if selected == FG_FSR4 {
            if !on(facts.fsr4_fg_compiled)
                || !on(facts.presenter)
                || !on(facts.interop_device_ready)
                || !on(facts.primary_window)
                || value.generated_frames != 1
            {
                return false;
            }
            // Refuse before asking the engine to retire its working WSI owner.
            if platform.vulkan_fsr4_fg_probe() < 1 {
                return false;
            }
        }
        if selected == FG_XESS
            && !(on(facts.xess_fg_compiled)
                && on(facts.interop_device_ready)
                && on(facts.primary_window))
        {
            return false;
        }
        if value.fg_mode != 0
            && (!on(facts.presenter) || (selected == FG_FSR3 && value.generated_frames != 1))
        {
            return false;
        }
        if value.fg_mode != 0 && facts.presenter_backend == selected {
            match platform.vulkan_presenter_max() {
                Some(maximum) if value.generated_frames <= maximum => {}
                _ => return false,
            }
        }
        true
    }

    pub(crate) fn settings(&self) -> (Settings, c_int) {
        (self.state().settings, c_int::from(self.booted()))
    }

    /// The record with the Streamline Vulkan latency overlay. Fills the record even when the
    /// session is not booted, as before.
    pub(crate) fn status(&self, platform: &Platform) -> (Status, c_int) {
        let mut value = self.state().status;
        if let Some(latency) = platform.vulkan_latency_facts() {
            apply_latency(&mut value, &latency);
        }
        (value, c_int::from(self.booted()))
    }

    /// Under the guard: what the render, presentation and capture leaves read.
    pub(crate) fn view(&self) -> (VkView, c_int) {
        let state = self.state();
        let view = VkView {
            settings: state.settings,
            sr_backend: sr_backend(&state.settings),
            generation: state.generation,
            session_id: state.status.session_id,
            fg_backend: fg_backend(&state.settings),
            output_width: state.status.output_width,
            output_height: state.status.output_height,
            fg_result: state.status.fg_result,
        };
        (view, c_int::from(self.booted()))
    }

    /// `rsfg_vk_create` finished. `maximum` is the presenter's capacity when it answered.
    pub(crate) fn presenter_created(&self, ready: bool, backend: u32, maximum: u32) {
        let mut state = self.state();
        let status = &mut state.status;
        status.fg_available = u32::from(ready);
        status.fg_backend = if ready { backend } else { 0 };
        status.max_generated_frames = if ready { maximum } else { 0 };
        if ready {
            state.presenter_generation = state.generation;
            state.guides = None;
        }
    }

    /// Whether a presenter owning `backend` may keep acquiring. A provider switch, or a new SR
    /// plan under a fixed-guide presenter, makes the engine drain and recreate its WSI owner.
    pub(crate) fn presenter_current(&self, backend: u32) -> bool {
        let state = self.state();
        backend == fg_backend(&state.settings)
            && !(fixed_guides(backend) && state.presenter_generation != state.generation)
    }

    pub(crate) fn presented(&self, facts: &VkPresentFacts) {
        let mut state = self.state();
        let frames = state.settings.generated_frames;
        let status = &mut state.status;
        status.fg_active = facts.enabled;
        status.fg_result = facts.result;
        status.generated_callbacks = facts.generated_callbacks;
        status.generated_frames = if facts.enabled != 0 { frames } else { 0 };
        status.total_presented = facts.presented;
        status.tagged_frames = facts.tagged;
    }

    /// The live presenter was destroyed.
    pub(crate) fn presenter_retired(&self) {
        let mut state = self.state();
        state.guides = None;
        let status = &mut state.status;
        status.fg_available = 0;
        status.fg_active = 0;
        status.fg_backend = 0;
        status.max_generated_frames = 0;
        status.generated_frames = 0;
    }

    /// FG runs only in requested gameplay on the flight screen.
    pub(crate) fn fg_begin(&self, pass: &mut VkFgPass) {
        let state = self.state();
        let settings = &state.settings;
        pass.enabled = u32::from(
            settings.fg_mode != 0 && settings.gameplay != 0 && pass.screen == SCREEN_FLIGHT,
        );
        pass.generated_frames = settings.generated_frames;
    }

    /// After the presenter took the enable options: whether to record this pass. A refusal
    /// stores its result.
    pub(crate) fn fg_admit(&self, pass: &VkFgPass) -> bool {
        let mut state = self.state();
        let frames = state.settings.generated_frames;
        if pass.backend_valid != 0 {
            state.status.fg_backend = pass.backend;
        }
        let options_ok = pass.options_result == BACKEND_OK;
        state.status.generated_frames = if pass.enabled != 0 && options_ok {
            frames
        } else {
            0
        };
        let fixed = fixed_guides(state.status.fg_backend);
        if fixed
            && pass.role == RENDER_SR
            && state.guides.is_some_and(|guides| {
                guides.session == pass.session_id
                    && guides.view == pass.view_id
                    && (guides.width != pass.render_width || guides.height != pass.render_height)
            })
        {
            // The SDK's first plan can settle one frame after settings change. Retire fixed
            // guide allocations through the normal WSI owner path.
            state.presenter_generation = 0;
        }
        let guides_ready = !fixed || state.presenter_generation == state.generation;
        if !options_ok {
            state.status.fg_result = pass.options_result;
            return false;
        }
        if !guides_ready {
            state.status.fg_result = BACKEND_ERROR_NOT_READY;
            return false;
        }
        true
    }

    /// The admitted pass was recorded with `result`.
    pub(crate) fn fg_recorded(&self, pass: &VkFgPass, result: i32) {
        let mut state = self.state();
        state.status.fg_result = result;
        if fixed_guides(state.status.fg_backend)
            && state.guides.is_none()
            && pass.role == RENDER_SR
            && result == BACKEND_OK
        {
            state.guides = Some(Guides {
                session: pass.session_id,
                view: pass.view_id,
                width: pass.render_width,
                height: pass.render_height,
            });
        }
    }

    /// One SR pass of `rsfg_render_vulkan`, after the provider, passthrough or fallback.
    pub(crate) fn sr_frame(&self, frame: &VkSrFrame) {
        let mut state = self.state();
        let requested = state.settings.sr_backend;
        let status = &mut state.status;
        if frame.identity != 0 {
            status.requested_sr = requested;
            status.output_width = frame.output_width;
            status.output_height = frame.output_height;
            status.render_width = frame.render_width;
            status.render_height = frame.render_height;
            status.source_frame_id = frame.source_frame_id;
        }
        status.sr_result = frame.result;
        let evaluated = frame.outcome == SR_EVALUATED && frame.result == BACKEND_OK;
        status.active_sr = if evaluated { frame.backend } else { 0 };
        if evaluated {
            status.evaluated_frames = status.evaluated_frames.wrapping_add(1);
        } else if frame.outcome == SR_REFUSED {
            status.refused_frames = status.refused_frames.wrapping_add(1);
        }
    }
}

fn apply_latency(status: &mut Status, latency: &VkLatencyFacts) {
    status.latency_available = latency.low_latency_available;
    status.latency_report_available = latency.report_available;
    status.pcl_available = u32::from(latency.pcl_message >= REGISTERED_MESSAGE);
    status.latency_mode = latency.mode;
    status.sleep_calls = latency.sleep_calls;
    status.marker_calls = latency.marker_calls;
    status.latency_owner = latency.owner;
}

#[cfg(test)]
mod tests;
