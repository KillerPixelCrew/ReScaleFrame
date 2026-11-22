// SPDX-License-Identifier: MIT
//! The GameSDK renderer lifecycle of ReScaleFrame.Game.AC7.dll.
//!
//! One lock guards the native renderer handle, the transition flag, the activation flag and the
//! status reason. A transition marks the lifecycle busy, calls its native leaf outside the lock
//! and publishes the outcome under the lock again, exactly as the former plugin.cpp did. The
//! native renderer (native_renderer.rs) verifies its bytes, owns every hook and reports native
//! facts; vendor, SR, FG and pacing decisions stay in the Runtime.
use crate::sys::{rsf_ac7_native_renderer, rsf_ac7_native_renderer_options};
use crate::{RSF_ERROR_BUSY, RSF_ERROR_NATIVE_REFUSED, RSF_ERROR_NOT_READY, RSF_OK};
use rsf_game_sys::{rsf_game_host_services, rsf_game_renderer_status, rsf_result};
use std::ffi::CStr;
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Queued render-scope capacity the adapter requests from the native pass controller.
pub(crate) const PENDING_CAPACITY: u32 = 256;

pub(crate) const NOT_PREPARED: &CStr = c"Native renderer has not been prepared.";
pub(crate) const PREPARED: &CStr = c"Native pass controller prepared; graphics activation pending.";
pub(crate) const REFUSED: &CStr =
    c"Native renderer refused: researched engine bytes are unavailable or differ.";
pub(crate) const STARTED: &CStr =
    c"Native view producer and pre-tonemap SR graph active; game validation pending.";
pub(crate) const START_REFUSED: &CStr = c"Native renderer activation refused.";
pub(crate) const QUIESCED: &CStr =
    c"Native producers quiesced; queued RHI work may still need to drain.";
pub(crate) const STOPPED: &CStr = c"Native renderer stopped.";
pub(crate) const DRAINING: &CStr =
    c"Native renderer draining; module and host services remain owned.";
pub(crate) const DEACTIVATED: &CStr =
    c"Native renderer deactivated after command ownership refusal.";

pub(crate) type Renderer = *mut rsf_ac7_native_renderer;

/// The native renderer entry points.
pub(crate) trait Leaves {
    /// The native output and whether preparation succeeded. The output is kept either way.
    fn prepare(&self, options: &rsf_ac7_native_renderer_options) -> (Renderer, bool);
    fn start(&self, renderer: Renderer) -> bool;
    fn is_active(&self, renderer: Renderer) -> bool;
    fn quiesce(&self, renderer: Renderer);
    fn stop(&self, renderer: Renderer) -> bool;
}

struct State {
    renderer: Renderer,
    transitioning: bool,
    prepared: bool,
    active: bool,
    reason: &'static CStr,
}

// SAFETY: the renderer handle is an opaque process-lived native object, used under the lock or
// by the single transition that owns the busy flag.
unsafe impl Send for State {}

pub(crate) struct Lifecycle(Mutex<State>);

/// The renderer options for a validated host. The renderer reads its startup diagnostics.
pub(crate) fn options(host: &rsf_game_host_services) -> rsf_ac7_native_renderer_options {
    rsf_ac7_native_renderer_options {
        struct_size: size_of::<rsf_ac7_native_renderer_options>() as u32,
        pending_capacity: PENDING_CAPACITY,
        session_id: host.session_id,
        on_stage: host.render_pass,
        log: host.log,
        user: host.user,
        render_config: host.render_config,
        cpu_event: host.cpu_event,
        on_state: host.scope_state,
        diagnostics: 0,
    }
}

impl Lifecycle {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(State {
            renderer: null_mut(),
            transitioning: false,
            prepared: false,
            active: false,
            reason: NOT_PREPARED,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `host` has passed the GameSDK checks and carries a nonzero session.
    pub(crate) fn prepare(
        &self,
        leaves: &impl Leaves,
        host: &rsf_game_host_services,
    ) -> rsf_result {
        {
            let mut state = self.lock();
            if !state.renderer.is_null() || state.transitioning {
                return RSF_ERROR_BUSY;
            }
            state.transitioning = true;
        }
        let (created, prepared) = leaves.prepare(&options(host));
        let mut state = self.lock();
        state.transitioning = false;
        state.renderer = created;
        state.prepared = prepared && !created.is_null();
        if state.prepared {
            state.reason = PREPARED;
            RSF_OK
        } else {
            state.reason = REFUSED;
            RSF_ERROR_NATIVE_REFUSED
        }
    }

    /// Marks the lifecycle busy, runs `work` on the renderer outside the lock, then publishes the
    /// outcome. `check` runs under the lock and may refuse; `finish` runs under the lock afterwards.
    fn transition(
        &self,
        check: impl FnOnce(&State) -> rsf_result,
        work: impl FnOnce(Renderer) -> bool,
        finish: impl FnOnce(&mut State, bool) -> rsf_result,
    ) -> rsf_result {
        let current = {
            let mut state = self.lock();
            if state.transitioning {
                return RSF_ERROR_BUSY;
            }
            let refused = check(&state);
            if refused != RSF_OK {
                return refused;
            }
            state.transitioning = true;
            state.renderer
        };
        let succeeded = work(current);
        let mut state = self.lock();
        state.transitioning = false;
        finish(&mut state, succeeded)
    }

    pub(crate) fn start(&self, leaves: &impl Leaves) -> rsf_result {
        self.transition(
            |state| {
                if !state.prepared {
                    RSF_ERROR_NOT_READY
                } else {
                    RSF_OK
                }
            },
            |current| leaves.start(current),
            |state, started| {
                state.active = started;
                if started {
                    state.reason = STARTED;
                    RSF_OK
                } else {
                    state.reason = START_REFUSED;
                    RSF_ERROR_NOT_READY
                }
            },
        )
    }

    pub(crate) fn quiesce(&self, leaves: &impl Leaves) -> rsf_result {
        self.transition(
            |_| RSF_OK,
            |current| {
                if !current.is_null() {
                    leaves.quiesce(current);
                }
                true
            },
            |state, _| {
                state.active = false;
                state.reason = QUIESCED;
                RSF_OK
            },
        )
    }

    pub(crate) fn stop(&self, leaves: &impl Leaves) -> rsf_result {
        self.transition(
            |state| if state.active { RSF_ERROR_BUSY } else { RSF_OK },
            |current| current.is_null() || leaves.stop(current),
            |state, stopped| {
                if stopped {
                    state.renderer = null_mut();
                    state.prepared = false;
                    state.reason = STOPPED;
                    RSF_OK
                } else {
                    state.reason = DRAINING;
                    RSF_ERROR_BUSY
                }
            },
        )
    }

    /// Fills the prepared, active, readiness and reason fields of a validated status.
    pub(crate) fn status(
        &self,
        leaves: &impl Leaves,
        output: &mut rsf_game_renderer_status,
    ) -> rsf_result {
        let state = self.lock();
        if state.transitioning {
            return RSF_ERROR_BUSY;
        }
        output.prepared = u32::from(state.prepared);
        // The native check runs only while the adapter believes the renderer active.
        let active = state.active && leaves.is_active(state.renderer);
        output.active = u32::from(active);
        output.rendering_ready = 0;
        output.reason = if state.active && !active {
            DEACTIVATED
        } else {
            state.reason
        }
        .as_ptr();
        RSF_OK
    }
}

#[cfg(test)]
mod tests;
