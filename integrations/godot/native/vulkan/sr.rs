// SPDX-License-Identifier: MIT
//! Godot retains the Runtime SR session until engine GPU use has retired. Runtime
//! owns the provider handle, recording continuation and spatial recovery.

use super::{SR_AUTO, SR_EVALUATED, SR_FSR4, SR_REFUSED, Vulkan};
use crate::d3d12::sharpening;
use crate::platform::{
    Platform, VK_LOG_FSR4_DISPATCH, VK_LOG_INPUT_REFUSAL, VK_LOG_REJECTED, VkProc, VkSrCreate,
    VkSrFrame, VkSrPass, VkView,
};
use rsf_game_sys::backend_result;
use rsf_game_sys::ffi_util::lock_or_recover;
use std::ffi::{c_int, c_void};
use std::sync::MutexGuard;

const SR_NONE: u32 = 0;
const SR_SPATIAL: u32 = 6;
/// Logged refusals per session: rejected passes, FSR4 dispatch failures and input refusals.
const DIAGNOSTIC_LIMIT: u32 = 4;

/// The live SR contexts and the request they were created for.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Contexts {
    session: usize,
    /// The last creation succeeded; a context kept after a failed creation is retired next pass.
    ready: bool,
    /// The engine device and its proc the contexts were created on, kept for retirement.
    device: usize,
    device_proc: Option<VkProc>,
    generation: u64,
    backend: u32,
    diagnostics: u32,
}

impl Contexts {
    pub(super) fn reset_diagnostics(&mut self) {
        self.diagnostics = 0;
    }

    /// After the session stopped with its contexts retired.
    pub(super) fn release_device(&mut self) {
        self.device = 0;
        self.device_proc = None;
    }

    fn live(&self) -> bool {
        self.session != 0
    }

    /// A failed creation, a new SR request, another resolved provider, another device, or an
    /// output other than the last reported pass's retires the contexts.
    fn stale(&self, view: &VkView, pass: &VkSrPass) -> bool {
        !self.ready
            || view.generation != self.generation
            || view.sr_backend != self.backend
            || pass.device as usize != self.device
            || view.output_width != pass.output_width
            || view.output_height != pass.output_height
    }
}

/// Counts one refusal against the budget; whether it may still be logged.
fn spend(diagnostics: &mut u32) -> bool {
    let within = *diagnostics < DIAGNOSTIC_LIMIT;
    *diagnostics = diagnostics.wrapping_add(1);
    within
}

/// After presenter drain, wait for the device before asking Runtime to close the SR session.
fn retire(contexts: &mut Contexts) -> bool {
    if !contexts.live() {
        return true;
    }
    if !Platform.vulkan_device_idle(contexts.device as *mut c_void, contexts.device_proc) {
        return false;
    }
    if Platform.vulkan_sr_destroy(contexts.session as *mut c_void) != backend_result::OK {
        return false;
    }
    contexts.session = 0;
    contexts.ready = false;
    true
}

impl Vulkan {
    pub(super) fn contexts(&self) -> MutexGuard<'_, Contexts> {
        lock_or_recover(&self.contexts)
    }

    pub(crate) fn sr_context(&self) -> *mut c_void {
        self.contexts().session as *mut c_void
    }

    /// The session stop's context retirement, after the presenter drained.
    pub(super) fn retire_contexts(&self) -> bool {
        let mut contexts = *self.contexts();
        let retired = retire(&mut contexts);
        *self.contexts() = contexts;
        retired
    }

    /// The SR section of `rsfg_render_vulkan`, called by `vulkan/native.rs` with the guard held after
    /// its argument, depth-domain and FG checks. Returns the export's result.
    pub(crate) fn sr_render(&self, pass: &VkSrPass) -> c_int {
        let view = self.view_for_device(Some(pass.device.addr())).0;
        let mut contexts = *self.contexts();
        let result = self.sr_pass(pass, &view, &mut contexts);
        *self.contexts() = contexts;
        result
    }

    fn sr_pass(&self, pass: &VkSrPass, view: &VkView, contexts: &mut Contexts) -> c_int {
        let call = pass.call;
        // Every outcome below is reported once; the session derives the status from it.
        let mut sr = VkSrFrame::default();
        let backend = view.sr_backend;
        if contexts.live()
            && contexts.stale(view, pass)
            && (Platform.vulkan_presenter_drain(false) != backend_result::OK || !retire(contexts))
        {
            sr.result = backend_result::NOT_READY;
            return self.record(call, &mut sr, contexts, &view.settings, false);
        }
        sr.identity = 1;
        sr.output_width = pass.output_width;
        sr.output_height = pass.output_height;
        sr.render_width = pass.render_width;
        sr.render_height = pass.render_height;
        sr.source_frame_id = pass.source_frame_id;
        // Auto with no servable provider: the engine rendered at Auto's reduced extent.
        if backend == SR_NONE && view.settings.sr_backend == SR_AUTO {
            sr.result = backend_result::NOT_SUPPORTED;
            return self.record(call, &mut sr, contexts, &view.settings, false);
        }
        // Godot evaluates FSR1 itself; no context passes the engine's output through.
        if backend == SR_NONE || backend == SR_SPATIAL {
            sr.backend = backend;
            sr.result = backend_result::OK;
            sr.outcome = SR_EVALUATED;
            self.sr_frame(&sr);
            return 1;
        }
        if !contexts.live() {
            contexts.ready = false;
            contexts.device = pass.device as usize;
            contexts.device_proc = pass.device_proc;
            let request = VkSrCreate {
                backend,
                quality: view.settings.quality,
                preset: view.settings.dlss_preset,
            };
            let created = Platform.vulkan_sr_create(call, &request);
            contexts.session = created.handle.addr();
            sr.result = created.result;
            if !contexts.live() || sr.result != backend_result::OK {
                return self.record(call, &mut sr, contexts, &view.settings, false);
            }
            contexts.generation = view.generation;
            contexts.backend = backend;
            contexts.ready = true;
        }
        let (width, height) = Platform.vulkan_sr_plan(call, contexts.session as *mut c_void);
        sr.render_width = width;
        sr.render_height = height;
        self.record(call, &mut sr, contexts, &view.settings, true)
    }

    fn record(
        &self,
        call: *mut c_void,
        sr: &mut VkSrFrame,
        contexts: &mut Contexts,
        settings: &crate::platform::Settings,
        evaluate: bool,
    ) -> c_int {
        let (session, sharpening) = if evaluate {
            (contexts.session as *mut c_void, sharpening(settings))
        } else {
            (std::ptr::null_mut(), crate::platform::Sharpening::default())
        };
        let (provider, output, commands) = Platform.vulkan_sr_record(call, session, &sharpening);
        if let Some(provider) = provider {
            sr.result = provider;
        }
        if evaluate {
            if contexts.backend == SR_FSR4
                && sr.result != backend_result::OK
                && spend(&mut contexts.diagnostics)
            {
                Platform.vulkan_sr_log(call, VK_LOG_FSR4_DISPATCH, sr.result, backend_result::OK);
            }
            Platform.vulkan_sr_dump_result(call, contexts.backend, sr.result);
            if sr.result == backend_result::INVALID_ARGUMENT && spend(&mut contexts.diagnostics) {
                Platform.vulkan_sr_log(call, VK_LOG_INPUT_REFUSAL, sr.result, backend_result::OK);
            }
        }
        if evaluate && sr.result == backend_result::OK && output == backend_result::OK {
            sr.backend = contexts.backend;
            sr.outcome = SR_EVALUATED;
        } else if !commands.is_null() {
            sr.outcome = SR_REFUSED;
            if spend(&mut contexts.diagnostics) {
                Platform.vulkan_sr_log(call, VK_LOG_REJECTED, sr.result, output);
            }
        }
        self.sr_frame(sr);
        c_int::from(output == backend_result::OK)
    }
}

#[cfg(test)]
mod tests;
