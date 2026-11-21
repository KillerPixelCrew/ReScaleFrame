// SPDX-License-Identifier: MIT
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Default)]
struct State {
    open: bool,
    current: usize,
    in_flight: usize,
    acquiring: bool,
}

/// The legacy current pointer. The orchestrator's latency ledger owns the
/// indexed frame associations, and the SDK owns the token objects.
pub(crate) struct FrameToken(Mutex<State>);

struct Lease<'a> {
    owner: &'a FrameToken,
    acquiring: bool,
    current: usize,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut state = self.owner.state();
        state.in_flight -= 1;
        if self.acquiring {
            state.acquiring = false;
        }
    }
}

impl FrameToken {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(State {
            open: false,
            current: 0,
            in_flight: 0,
            acquiring: false,
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn open(&self) {
        self.state().open = true;
    }

    pub(crate) fn try_close(&self) -> bool {
        let mut state = self.state();
        if state.in_flight != 0 {
            return false;
        }
        *state = State::default();
        true
    }

    fn lease(&self, acquiring: bool, inactive: i32) -> Result<Lease<'_>, i32> {
        let mut state = self.state();
        if !state.open {
            return Err(inactive);
        }
        if acquiring && state.acquiring {
            // A recursive or overlapping begin must not reorder publication.
            return Err(crate::lifecycle::BUSY);
        }
        state.acquiring |= acquiring;
        state.in_flight += 1;
        Ok(Lease {
            owner: self,
            acquiring,
            current: state.current,
        })
    }

    pub(crate) fn begin(&self, acquire: impl FnOnce(&mut usize) -> i32) -> i32 {
        let _lease = match self.lease(true, -1) {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let mut token = 0;
        let result = acquire(&mut token);
        if result == 0 {
            self.state().current = token;
        }
        result
    }

    pub(crate) fn current(&self) -> usize {
        self.state().current
    }

    pub(crate) fn with_token(
        &self,
        explicit: usize,
        inactive: i32,
        call: impl FnOnce(usize) -> i32,
    ) -> i32 {
        let Ok(lease) = self.lease(false, inactive) else {
            return inactive;
        };
        let token = if explicit == 0 {
            lease.current
        } else {
            explicit
        };
        if token == 0 {
            return inactive;
        }
        // Only the lease spans the SDK call. Present may use an older token
        // while the current one is in Reflex sleep, including SDK reentry.
        call(token)
    }
}
