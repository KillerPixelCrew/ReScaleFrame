// SPDX-License-Identifier: MIT
//! The GameSDK renderer lifecycle of ReScaleFrame.Game.AC7.dll.
//!
//! One lock guards the native renderer handle, the transition flag, the activation flag and the
//! status reason. A transition marks the lifecycle busy, calls the native renderer outside the
//! lock and publishes the outcome under the lock again, exactly as the former plugin.cpp did. The
//! native renderer (native_renderer.rs) verifies its bytes, owns every hook and reports native
//! facts; vendor, SR, FG and pacing decisions stay in the Runtime.
use crate::native_renderer::transaction::{self as native, Handle};
use crate::sys::rsf_ac7_native_renderer_options;
use crate::{RSF_ERROR_BUSY, RSF_ERROR_NATIVE_REFUSED, RSF_ERROR_NOT_READY, RSF_OK};
use rsf_game_sys::{rsf_game_host_services, rsf_game_renderer_status, rsf_result};
use std::ffi::CStr;
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard, PoisonError};

const NOT_PREPARED: &CStr = c"Native renderer has not been prepared.";
const PREPARED: &CStr = c"Native pass controller prepared; graphics activation pending.";
const REFUSED: &CStr =
    c"Native renderer refused: researched engine bytes are unavailable or differ.";
const STARTED: &CStr =
    c"Native view producer and pre-tonemap SR graph active; game validation pending.";
const START_REFUSED: &CStr = c"Native renderer activation refused.";
const QUIESCED: &CStr = c"Native producers quiesced; queued RHI work may still need to drain.";
const STOPPED: &CStr = c"Native renderer stopped.";
const DRAINING: &CStr = c"Native renderer draining; module and host services remain owned.";
const DEACTIVATED: &CStr = c"Native renderer deactivated after command ownership refusal.";

struct State {
    renderer: Handle,
    transitioning: bool,
    /// Preparation succeeded. A refused preparation can still hand back a pinned renderer, kept
    /// only so stop can retire it; it reports unprepared and refuses start.
    prepared: bool,
    active: bool,
    reason: &'static CStr,
}

// SAFETY: the renderer handle is an opaque process-lived native object, used under the lock or
// by the single transition that owns the busy flag.
unsafe impl Send for State {}

pub(crate) struct Lifecycle(Mutex<State>);

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
    pub(crate) fn prepare(&self, host: &rsf_game_host_services) -> rsf_result {
        {
            let mut state = self.lock();
            if !state.renderer.is_null() || state.transitioning {
                return RSF_ERROR_BUSY;
            }
            state.transitioning = true;
        }
        let (created, prepared) = native::prepare(&rsf_ac7_native_renderer_options {
            session_id: host.session_id,
            on_stage: host.render_pass,
            log: host.log,
            user: host.user,
            render_config: host.render_config,
            cpu_event: host.cpu_event,
            on_state: host.scope_state,
        });
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
        work: impl FnOnce(Handle) -> bool,
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

    pub(crate) fn start(&self) -> rsf_result {
        self.transition(
            |state| {
                if state.prepared {
                    RSF_OK
                } else {
                    RSF_ERROR_NOT_READY
                }
            },
            native::start,
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

    pub(crate) fn quiesce(&self) -> rsf_result {
        self.transition(
            |_| RSF_OK,
            |current| {
                native::quiesce(current);
                true
            },
            |state, _| {
                state.active = false;
                state.reason = QUIESCED;
                RSF_OK
            },
        )
    }

    pub(crate) fn stop(&self) -> rsf_result {
        self.transition(
            |state| if state.active { RSF_ERROR_BUSY } else { RSF_OK },
            native::stop,
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
    pub(crate) fn status(&self, output: &mut rsf_game_renderer_status) -> rsf_result {
        let state = self.lock();
        if state.transitioning {
            return RSF_ERROR_BUSY;
        }
        output.prepared = u32::from(state.prepared);
        // The native check runs only while the adapter believes the renderer active.
        let active = state.active && native::is_active(state.renderer);
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
