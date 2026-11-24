// SPDX-License-Identifier: MIT
//! Godot Vulkan settings, provider capability snapshots and pass status. Runtime
//! selects Auto providers and owns the reconstruction and presentation mechanisms.
//! State transitions run under the native graphics guard, with no callbacks while
//! a Rust state lock is held.

use crate::control::default_settings;
use crate::platform::{
    ABI_VERSION, Platform, Settings, Setup, Status, VkFacts, VkLatencyFacts, VkPresentFacts,
    VkSrFrame, VkView,
};
use rsf_game_sys::backend_result;
use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const SR_DLSS: u32 = 1;
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
/// The first registered window message; the PCL ping message is one when Streamline bound it.
const REGISTERED_MESSAGE: u32 = 0xc000;
/// A Vulkan handle from the pointer-sized word the leaf records carry.
pub(crate) fn handle<H: ash::vk::Handle>(pointer: *mut c_void) -> H {
    H::from_raw(pointer.addr() as u64)
}

/// How an SR pass was counted.
pub(crate) const SR_EVALUATED: u32 = 1;
pub(crate) const SR_REFUSED: u32 = 2;

/// Bit `id` of a Runtime choices mask when `servable`.
fn choice(servable: bool, id: u32) -> u32 {
    if servable { 1 << id } else { 0 }
}

/// The requested SR and FG providers with Auto resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Providers {
    sr: u32,
    fg: u32,
}

struct Capabilities {
    facts: VkFacts,
    device: creation::DeviceReceipt,
    primary: bool,
}

impl Capabilities {
    fn sr_choices(&self) -> u32 {
        choice(true, SR_FSR3)
            | choice(self.facts.streamline_ready != 0, SR_DLSS)
            | choice(self.device.xess, SR_XESS)
            | choice(
                self.facts.fsr4_sr_compiled != 0
                    && self.facts.graph_handoff != 0
                    && self.device.interop,
                SR_FSR4,
            )
    }

    fn fg_choices(&self) -> u32 {
        choice(true, FG_FSR3)
            | choice(
                self.facts.xess_fg_compiled != 0 && self.device.interop && self.primary,
                FG_XESS,
            )
    }
}

struct State {
    settings: Settings,
    generation: u64,
    status: Status,
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
    /// The engine's primary window as `rsfg_vk_window` last reported it, admitted or not: the FG
    /// presenter's host window and the FSR4 and XeSS FG precondition. Null from each start.
    primary_window: AtomicPtr<c_void>,
    state: Mutex<State>,
    contexts: Mutex<sr::Contexts>,
    /// The device creation owner: the resolver, its requirements and the XeSS and interop
    /// readiness `admit` reads.
    creation: creation::Creation,
}

impl Vulkan {
    pub(crate) fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            primary_window: AtomicPtr::new(ptr::null_mut()),
            state: Mutex::new(State {
                settings: default_settings(),
                generation: 1,
                status: initial_status(),
            }),
            contexts: Mutex::new(sr::Contexts::default()),
            creation: creation::Creation::new(),
        }
    }

    pub(crate) fn creation(&self) -> &creation::Creation {
        &self.creation
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `settings`' providers with Auto resolved by the Runtime's adapter policy
    /// (`rsf_gpu_select_sr_backend`; `rsf_fg_select_auto_backend` of `rsf_fg_default_backend`) for
    /// the physical device the engine's last device was created on, among the providers this
    /// session can serve now.
    /// Explicit requests pass unchanged and read no facts. `facts` are the guarded owner facts
    /// when the caller already read them. Runs under the guard; the state lock is not held.
    ///
    /// The choices are the admission's own facts, so Auto never selects a provider `admit` would
    /// refuse: DLSS SR needs the Streamline session started with DLSS SR and FG; XeSS SR the device
    /// created with the XeSS requirements; FSR4 SR its build, the engine's graph lease capability
    /// and the Win32 interop device; XeSS FG its build, the interop device and the primary window.
    /// FSR3 is always served. Two FG providers are not offered to Auto. DLSS-G: its presenter's
    /// retirement after native secondary window use still ends in an access violation (NATIVE.md,
    /// eighth slice), so it stays an explicit opt-in until that path is proven safe. FSR4 FG: its
    /// admission asks a provider probe, which Auto does not trigger, and the Runtime never prefers
    /// it. Without adapter facts (no device yet, or the properties read failed) vendor and software
    /// are zero, the Runtime's unknown adapter.
    fn providers(&self, settings: &Settings, capabilities: Option<&Capabilities>) -> Providers {
        let auto_sr = settings.sr_backend == SR_AUTO;
        let auto_fg = settings.fg_backend == FG_BACKEND_AUTO;
        let mut providers = Providers {
            sr: settings.sr_backend,
            fg: settings.fg_backend,
        };
        if !auto_sr && !auto_fg {
            return providers;
        }
        let snapshot;
        let capabilities = match capabilities {
            Some(capabilities) => capabilities,
            None => {
                snapshot = self.capabilities();
                &snapshot
            }
        };
        let adapter = capabilities.device.adapter.unwrap_or_default();
        if auto_sr {
            providers.sr = Platform.sr_select_backend(
                adapter.vendor,
                adapter.software,
                capabilities.sr_choices(),
            );
        }
        if auto_fg {
            providers.fg = Platform.fg_select_backend(
                adapter.vendor,
                adapter.software,
                capabilities.fg_choices(),
            );
        }
        providers
    }

    pub(crate) fn booted(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn primary_window(&self) -> *mut c_void {
        self.primary_window.load(Ordering::Acquire)
    }

    /// `rsfg_vk_window` while booted, after the input window admission was asked.
    pub(crate) fn report_window(&self, window: *mut c_void) {
        self.primary_window.store(window, Ordering::Release);
    }

    /// `setup` passed the size and ABI checks; its strings are borrowed for the call.
    pub(crate) fn boot(&self, setup: &Setup) -> c_int {
        if setup.runtime_directory.is_null() || !Platform.vulkan_supported() {
            return 0;
        }
        Platform.vulkan_locked(|| {
            if self.booted() || Platform.vulkan_facts().sdk_shutdown_pending != 0 {
                return 0;
            }
            {
                // The generation keeps counting across sessions.
                let mut state = self.state();
                state.settings = default_settings();
                state.status = initial_status();
                state.status.session_id = Platform.tick_count().wrapping_add(1);
                state.status.fg_result = backend_result::NOT_SUPPORTED;
            }
            // Each session logs its own refusals.
            self.contexts().reset_diagnostics();
            // The start leaf cleared the primary window first, also when it then refused.
            self.primary_window
                .store(ptr::null_mut(), Ordering::Release);
            if Platform.vulkan_start(setup) == 0 {
                return 0;
            }
            self.active.store(true, Ordering::Release);
            1
        })
    }

    /// The caller found the session booted. The presenter is disabled and drained, then the SR
    /// contexts retire before the capture, XeSS and SDK stop. A refused step keeps the session
    /// booted.
    pub(crate) fn shutdown(&self) -> c_int {
        Platform.vulkan_locked(|| {
            if !self.booted() {
                return 1;
            }
            if Platform.vulkan_presenter_drain(true) != backend_result::OK
                || !self.retire_contexts()
                || Platform.vulkan_stop() == 0
            {
                return 0;
            }
            self.active.store(false, Ordering::Release);
            let session = {
                let mut state = self.state();
                state.status.active_sr = 0;
                state.status.sr_result = backend_result::NOT_READY;
                state.status.session_id
            };
            // The device borrow ends with the session.
            self.contexts().release_device();
            Platform.vulkan_release(session);
            1
        })
    }

    /// `settings` passed the shared `rsfg_configure` refusal rules.
    pub(crate) fn configure(&self, settings: &Settings) -> c_int {
        if !self.booted() {
            return 0;
        }
        Platform.vulkan_locked(|| {
            if !self.booted() || !self.admit(settings) {
                return 0;
            }
            if Platform.vulkan_latency_options(settings.latency_mode, settings.frame_limit_us)
                != backend_result::OK
            {
                return 0;
            }
            let mut state = self.state();
            if state.settings.reconstruction_key() != settings.reconstruction_key() {
                state.generation = state.generation.wrapping_add(1);
            }
            state.settings = *settings;
            1
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            facts: Platform.vulkan_facts(),
            device: self.creation.render_receipt(),
            primary: !self.primary_window().is_null(),
        }
    }

    /// Device readiness and provider refusals, in the order `vulkan.cpp` applied them. Runs under
    /// the guard; the FSR4 FG probe and the presenter capacity are asked only when reached.
    fn admit(&self, value: &Settings) -> bool {
        let capabilities = self.capabilities();
        let facts = capabilities.facts;
        let on = |flag: u32| flag != 0;
        // DLSS admission is still verified by its live provider at evaluation.
        if matches!(value.sr_backend, SR_XESS | SR_FSR4)
            && capabilities.sr_choices() & choice(true, value.sr_backend) == 0
        {
            return false;
        }
        let selected = self.providers(value, Some(&capabilities)).fg;
        if !matches!(selected, FG_DLSS | FG_FSR3 | FG_FSR4 | FG_XESS) {
            return false;
        }
        if selected == FG_FSR4 {
            if !on(facts.fsr4_fg_compiled)
                || !on(facts.presenter)
                || !capabilities.device.interop
                || !capabilities.primary
                || value.generated_frames != 1
            {
                return false;
            }
            // Refuse before asking the engine to retire its working WSI owner.
            if Platform.vulkan_fsr4_fg_probe() < 1 {
                return false;
            }
        }
        if selected == FG_XESS && capabilities.fg_choices() & choice(true, selected) == 0 {
            return false;
        }
        if value.fg_mode != 0
            && (!on(facts.presenter) || (selected == FG_FSR3 && value.generated_frames != 1))
        {
            return false;
        }
        if value.fg_mode != 0 && facts.presenter_backend == selected {
            match Platform.vulkan_presenter_max() {
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
    pub(crate) fn status(&self) -> (Status, c_int) {
        let mut value = self.state().status;
        if let Some(latency) = Platform.vulkan_latency_facts() {
            apply_latency(&mut value, &latency);
        }
        (value, c_int::from(self.booted()))
    }

    /// Under the guard: what the render, presentation and capture leaves read. Auto is resolved
    /// from one settings snapshot after the state lock is released.
    pub(crate) fn view(&self) -> (VkView, c_int) {
        self.view_for_device(None)
    }

    fn view_for_device(&self, device: Option<usize>) -> (VkView, c_int) {
        let mut view = {
            let state = self.state();
            VkView {
                settings: state.settings,
                sr_backend: state.settings.sr_backend,
                generation: state.generation,
                session_id: state.status.session_id,
                fg_backend: state.settings.fg_backend,
                output_width: state.status.output_width,
                output_height: state.status.output_height,
            }
        };
        let capabilities =
            if view.settings.sr_backend == SR_AUTO || view.settings.fg_backend == FG_BACKEND_AUTO {
                device.map(|device| Capabilities {
                    device: self.creation.device_receipt(device),
                    facts: Platform.vulkan_facts(),
                    primary: !self.primary_window().is_null(),
                })
            } else {
                None
            };
        let providers = self.providers(&view.settings, capabilities.as_ref());
        view.sr_backend = providers.sr;
        view.fg_backend = providers.fg;
        (view, c_int::from(self.booted()))
    }

    /// `rsfg_vk_create` finished. `maximum` is the presenter's capacity when it answered.
    pub(crate) fn presenter_created(&self, ready: bool, backend: u32, maximum: u32) {
        let mut state = self.state();
        let status = &mut state.status;
        status.fg_available = u32::from(ready);
        status.fg_backend = if ready { backend } else { 0 };
        status.max_generated_frames = if ready { maximum } else { 0 };
    }

    /// Provider changes still require the engine to recreate its WSI owner.
    /// The Runtime presenter manages guide sizes at retired slot reuse.
    pub(crate) fn presenter_current(&self, backend: u32) -> bool {
        let settings = self.state().settings;
        backend == self.providers(&settings, None).fg
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
        let status = &mut state.status;
        status.fg_available = 0;
        status.fg_active = 0;
        status.fg_backend = 0;
        status.max_generated_frames = 0;
        status.generated_frames = 0;
    }

    /// Applies the engine's gameplay facts around one synchronous Runtime recording.
    pub(crate) fn fg_pass(
        &self,
        screen: u32,
        presenter: bool,
        options: impl FnOnce(u32, u32) -> i32,
        record: impl FnOnce() -> i32,
    ) -> i32 {
        let settings = self.state().settings;
        let enabled =
            u32::from(settings.fg_mode != 0 && settings.gameplay != 0 && screen == SCREEN_FLIGHT);
        let options_result = if presenter {
            options(enabled, settings.generated_frames)
        } else {
            backend_result::NOT_READY
        };
        let result = if options_result == backend_result::OK {
            record()
        } else {
            options_result
        };
        let mut state = self.state();
        state.status.generated_frames = if enabled != 0 && options_result == backend_result::OK {
            settings.generated_frames
        } else {
            0
        };
        state.status.fg_result = result;
        result
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
        let evaluated = frame.outcome == SR_EVALUATED && frame.result == backend_result::OK;
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

pub(crate) mod creation;
mod sr;

#[cfg(test)]
mod tests;

#[cfg(all(windows, not(test)))]
pub(crate) mod native;
