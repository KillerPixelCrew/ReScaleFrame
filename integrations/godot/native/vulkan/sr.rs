// SPDX-License-Identifier: MIT
//! The Vulkan SR context owner formerly held by `vulkan.cpp`'s `rsfg_render_vulkan`: the live
//! `rsf_srvk` and FSR4 interop contexts, the device, generation and provider they were made for,
//! their retirement (stale request, device or output, and the session stop) and recreation, the
//! provider admission for FSR4 (build fact and graph handoff lease), the DLSS preset, the plan,
//! the evaluate dispatch with the FSR4 continuation, the spatial fallback, the refusal diagnostics
//! budget and the SR outcome reported to the session.
//!
//! `vulkan.cpp` keeps every typed call as a leaf: the presenter drain, `vkDeviceWaitIdle`,
//! `rsf_srvk_*`, `rsf_fsr4vk_*`, the spatial fallback, the frame and setup built from the packet,
//! the frame dump result and the log text. The context handles are held here as addresses and
//! lent to each leaf. Every path here runs while `vulkan.cpp` holds its guard; the contexts lock
//! is never held across a leaf.
//!
//! Auto SR stays FSR3 as before (`super::sr_backend`). The Runtime's `rsf_gpu_select_sr_backend`
//! would match it only on a known hardware adapter and needs the Vulkan vendor id, which the
//! session does not have when it resolves Auto; see `NATIVE.md`.
use super::{BACKEND_ERROR_NOT_READY, BACKEND_OK, SR_EVALUATED, SR_FSR4, SR_REFUSED, Vulkan};
use crate::d3d12::sharpening;
use crate::platform::{
    Platform, VK_LOG_FSR4_DISPATCH, VK_LOG_INPUT_REFUSAL, VK_LOG_REJECTED, VkProc, VkSrCreate,
    VkSrFrame, VkSrPass, VkView,
};
use std::ffi::{c_int, c_void};
use std::sync::{MutexGuard, PoisonError};

const BACKEND_ERROR_INVALID_ARGUMENT: i32 = -1;
const BACKEND_ERROR_NOT_COMPILED: i32 = -3;
const SR_NONE: u32 = 0;
const SR_DLSS: u32 = 1;
const SR_SPATIAL: u32 = 6;
/// Logged refusals per session: rejected passes, FSR4 dispatch failures and input refusals.
const DIAGNOSTIC_LIMIT: u32 = 4;

/// The live SR contexts and the request they were created for.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Contexts {
    /// `rsf_srvk` handle, as an address.
    sr: usize,
    /// `rsf_fsr4vk` handle, as an address.
    fsr4: usize,
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
        self.sr != 0 || self.fsr4 != 0
    }

    /// A failed creation, a new SR request, another device, or an output other than the last
    /// reported pass's retires the contexts.
    fn stale(&self, view: &VkView, pass: &VkSrPass) -> bool {
        !self.ready
            || view.generation != self.generation
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

/// Drains the presenter, waits for the contexts' device and destroys FSR4, then `rsf_srvk`. A
/// refusal keeps what was not destroyed.
fn retire(platform: &Platform, contexts: &mut Contexts) -> bool {
    if !contexts.live() {
        return true;
    }
    if platform.vulkan_presenter_drain(false) != BACKEND_OK
        || !platform.vulkan_device_idle(contexts.device as *mut c_void, contexts.device_proc)
    {
        return false;
    }
    if contexts.fsr4 != 0 && platform.vulkan_fsr4_destroy(contexts.fsr4 as *mut c_void) != 0 {
        return false;
    }
    contexts.fsr4 = 0;
    if contexts.sr != 0 && platform.vulkan_sr_destroy(contexts.sr as *mut c_void) != BACKEND_OK {
        return false;
    }
    contexts.sr = 0;
    contexts.ready = false;
    true
}

impl Vulkan {
    pub(super) fn contexts(&self) -> MutexGuard<'_, Contexts> {
        self.contexts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The live handles for `rsfg_vk_recording_begin`, null when none.
    pub(crate) fn sr_contexts(&self) -> (*mut c_void, *mut c_void) {
        let contexts = *self.contexts();
        (contexts.sr as *mut c_void, contexts.fsr4 as *mut c_void)
    }

    /// The session stop's context retirement, after the presenter drained.
    pub(super) fn retire_contexts(&self, platform: &Platform) -> bool {
        let mut contexts = *self.contexts();
        let retired = retire(platform, &mut contexts);
        *self.contexts() = contexts;
        retired
    }

    /// The SR section of `rsfg_render_vulkan`, called by `vulkan.cpp` with the guard held after
    /// its argument, depth-domain and FG checks. Returns the export's result; a leaf exception
    /// returns 0 without a report, as the export's catch did.
    pub(crate) fn sr_render(&self, platform: &Platform, pass: &VkSrPass) -> c_int {
        let view = self.view().0;
        let mut contexts = *self.contexts();
        let result = self.sr_pass(platform, pass, &view, &mut contexts);
        *self.contexts() = contexts;
        result.unwrap_or(0)
    }

    fn sr_pass(
        &self,
        platform: &Platform,
        pass: &VkSrPass,
        view: &VkView,
        contexts: &mut Contexts,
    ) -> Option<c_int> {
        let call = pass.call;
        // Every outcome below is reported once; the session derives the status from it.
        let mut sr = VkSrFrame::default();
        let mut commands = pass.commands;
        let backend = view.sr_backend;
        if contexts.live() && contexts.stale(view, pass) && !retire(platform, contexts) {
            sr.result = BACKEND_ERROR_NOT_READY;
            return self.fallback(platform, call, commands, &mut sr, contexts);
        }
        sr.identity = 1;
        sr.output_width = pass.output_width;
        sr.output_height = pass.output_height;
        sr.render_width = pass.render_width;
        sr.render_height = pass.render_height;
        sr.source_frame_id = pass.source_frame_id;
        // Godot evaluates FSR1 itself; no context passes the engine's output through.
        if backend == SR_NONE || backend == SR_SPATIAL {
            sr.backend = backend;
            sr.result = BACKEND_OK;
            sr.outcome = SR_EVALUATED;
            self.sr_frame(&sr);
            return Some(1);
        }
        if !contexts.live() {
            contexts.ready = false;
            contexts.device = pass.device as usize;
            contexts.device_proc = pass.device_proc;
            let request = VkSrCreate {
                backend,
                quality: view.settings.quality,
            };
            if backend == SR_FSR4 {
                if pass.fsr4_compiled == 0 {
                    sr.result = BACKEND_ERROR_NOT_COMPILED;
                } else if pass.handoff == 0 || pass.split_record == 0 || pass.continuation == 0 {
                    // The interop context needs the graph lease to split its recording.
                    sr.result = BACKEND_ERROR_NOT_READY;
                    return self.fallback(platform, call, commands, &mut sr, contexts);
                } else {
                    let (completed, created) = platform.vulkan_sr_create(call, &request, true);
                    contexts.fsr4 = created.handle as usize;
                    if !completed {
                        return None;
                    }
                    sr.result = created.result;
                }
            } else {
                let (completed, created) = platform.vulkan_sr_create(call, &request, false);
                contexts.sr = created.handle as usize;
                if !completed {
                    return None;
                }
                sr.result = created.result;
            }
            if !contexts.live() || sr.result != BACKEND_OK {
                return self.fallback(platform, call, commands, &mut sr, contexts);
            }
            if backend == SR_DLSS {
                platform
                    .vulkan_dlss_preset(contexts.sr as *mut c_void, view.settings.dlss_preset)?;
            }
            contexts.device = pass.device as usize;
            contexts.device_proc = pass.device_proc;
            contexts.generation = view.generation;
            contexts.backend = backend;
            contexts.ready = true;
        }
        let fsr4 = contexts.fsr4 != 0;
        let planned = if fsr4 { contexts.fsr4 } else { contexts.sr };
        let (width, height) = platform.vulkan_sr_plan(call, planned as *mut c_void, fsr4)?;
        sr.render_width = width;
        sr.render_height = height;
        let sharpening = sharpening(platform, &view.settings);
        if backend == SR_FSR4 {
            if pass.fsr4_compiled == 0 {
                sr.result = BACKEND_ERROR_NOT_COMPILED;
            } else if pass.handoff == 0 || pass.continuation == 0 {
                sr.result = BACKEND_ERROR_NOT_READY;
                return self.fallback(platform, call, commands, &mut sr, contexts);
            } else {
                let (result, next) = platform.vulkan_fsr4_evaluate(
                    call,
                    contexts.fsr4 as *mut c_void,
                    &sharpening,
                )?;
                // Recording continues on the engine's continuation, which may be none.
                commands = next;
                sr.result = result;
                if result != BACKEND_OK && spend(&mut contexts.diagnostics) {
                    platform.vulkan_sr_log(call, VK_LOG_FSR4_DISPATCH, result, BACKEND_OK)?;
                }
            }
        } else {
            sr.result =
                platform.vulkan_sr_evaluate(call, contexts.sr as *mut c_void, &sharpening)?;
        }
        platform.vulkan_sr_dump_result(call, contexts.backend, sr.result)?;
        if sr.result == BACKEND_ERROR_INVALID_ARGUMENT && spend(&mut contexts.diagnostics) {
            platform.vulkan_sr_log(call, VK_LOG_INPUT_REFUSAL, sr.result, BACKEND_OK)?;
        }
        if sr.result != BACKEND_OK {
            return self.fallback(platform, call, commands, &mut sr, contexts);
        }
        sr.backend = contexts.backend;
        sr.outcome = SR_EVALUATED;
        self.sr_frame(&sr);
        Some(1)
    }

    /// The spatial fallback on the current command buffer. Without one (an FSR4 dispatch that
    /// left no continuation) nothing is recorded and the pass is not counted.
    fn fallback(
        &self,
        platform: &Platform,
        call: *mut c_void,
        commands: *mut c_void,
        sr: &mut VkSrFrame,
        contexts: &mut Contexts,
    ) -> Option<c_int> {
        if commands.is_null() {
            self.sr_frame(sr);
            return Some(0);
        }
        let result = platform.vulkan_sr_fallback(call)?;
        sr.outcome = SR_REFUSED;
        if spend(&mut contexts.diagnostics) {
            platform.vulkan_sr_log(call, VK_LOG_REJECTED, sr.result, result)?;
        }
        self.sr_frame(sr);
        Some(c_int::from(result == BACKEND_OK))
    }
}

#[cfg(test)]
mod tests;
