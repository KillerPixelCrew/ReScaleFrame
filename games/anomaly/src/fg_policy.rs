// SPDX-License-Identifier: MIT
use crate::fg_policy_abi::*;
use std::sync::Mutex;

type Decision = rsf_anomaly_fg_decision;
type Counters = rsf_anomaly_fg_counters;
type History = Mutex<Policy>;

const GAMEPLAY: u32 = RSF_ANOMALY_GAMEPLAY_SCREEN as u32;
const HELD: u32 = RSF_ANOMALY_HELD_SCREEN as u32;

const _: fn(&Counters) = |counts| {
    let _: &[u64; RSF_ANOMALY_SKIP_COUNT as usize] = &counts.skipped;
    let _: &[u64; RSF_ANOMALY_RESET_COUNT as usize] = &counts.resets;
};

#[derive(Default)]
struct Frame {
    level: bool,
    scope: bool,
    presented: bool,
    extent: [u32; 2],
    decision: Decision,
}

pub(crate) struct Policy {
    scope_hold: u32,
    engine_frame: u32,
    last_scope: Option<u32>,
    scoped: bool,
    first: bool,
    previous_fed: bool,
    pending: i32,
    fed_extent: [u32; 2],
    frame: Frame,
    counts: Counters,
}

impl Policy {
    pub(crate) fn new(scope_hold: u32) -> Self {
        Self {
            scope_hold,
            engine_frame: 0,
            last_scope: None,
            scoped: false,
            first: true,
            previous_fed: false,
            pending: RSF_ANOMALY_RESET_NONE,
            fed_extent: [0; 2],
            frame: Frame::default(),
            counts: Counters::default(),
        }
    }

    #[cfg(windows)]
    pub(crate) fn scope_hold(&self) -> u32 {
        self.scope_hold
    }

    #[cfg(windows)]
    pub(crate) fn set_scope_hold(&mut self, frames: u32) {
        self.scope_hold = frames;
    }

    pub(crate) fn begin(&mut self, engine_frame: u32, extent: [u32; 2], cut: bool) -> Decision {
        self.engine_frame = engine_frame;
        self.counts.source_frames += 1;
        let mut decision = Decision {
            tag: 1,
            screen: GAMEPLAY,
            ..Default::default()
        };
        if self
            .last_scope
            .is_some_and(|last| engine_frame.wrapping_sub(last) <= self.scope_hold)
        {
            decision.skip = RSF_ANOMALY_SKIP_SCOPE;
            decision.screen = HELD;
        } else {
            decision.reset = if self.first {
                RSF_ANOMALY_RESET_FIRST
            } else if self.pending != RSF_ANOMALY_RESET_NONE {
                self.pending
            } else if self.scoped {
                RSF_ANOMALY_RESET_SCOPE_END
            } else if !self.previous_fed {
                RSF_ANOMALY_RESET_DISCONTINUITY
            } else if extent != self.fed_extent {
                RSF_ANOMALY_RESET_SIZE
            } else if cut {
                RSF_ANOMALY_RESET_CUT
            } else {
                RSF_ANOMALY_RESET_NONE
            };
        }
        self.frame = Frame {
            extent,
            decision,
            ..Default::default()
        };
        decision
    }

    pub(crate) fn end(&mut self) -> Decision {
        let frame = &mut self.frame;
        let decision = &mut frame.decision;
        if !frame.level {
            decision.skip = RSF_ANOMALY_SKIP_NO_LEVEL;
        } else if decision.skip == RSF_ANOMALY_SKIP_NONE && !frame.presented {
            decision.skip = RSF_ANOMALY_SKIP_NO_PRESENT;
        }
        if frame.presented {
            self.counts.presented += 1;
        } else {
            self.counts.unpresented += 1;
        }
        self.counts.skipped[decision.skip as usize] += 1;
        let fed = decision.skip == RSF_ANOMALY_SKIP_NONE;
        if fed {
            self.counts.fed += 1;
            self.counts.resets[decision.reset as usize] += 1;
            self.first = false;
            self.pending = RSF_ANOMALY_RESET_NONE;
            self.scoped = false;
            self.fed_extent = frame.extent;
        } else if decision.skip == RSF_ANOMALY_SKIP_SCOPE {
            self.scoped = true;
        }
        self.previous_fed = fed;
        *decision
    }

    pub(crate) fn device_reset(&mut self) {
        if self.pending == RSF_ANOMALY_RESET_NONE {
            self.pending = RSF_ANOMALY_RESET_DEVICE;
        }
        // A reset can follow Present before the engine ends this frame.
        // That presentation no longer establishes history on the new device.
        self.frame.presented = false;
    }

    #[cfg(windows)]
    pub(crate) fn invalidate_history(&mut self) {
        self.previous_fed = false;
    }

    pub(crate) fn scope(&mut self) {
        if !self.frame.scope {
            self.counts.scope_frames += 1;
        }
        self.frame.scope = true;
        self.last_scope = Some(self.engine_frame);
        self.frame.decision.skip = RSF_ANOMALY_SKIP_SCOPE;
        self.frame.decision.screen = HELD;
        self.frame.decision.tag = 0;
    }

    pub(crate) fn ui_boundary(&mut self) -> Decision {
        let decision = self.frame.decision;
        if !self.frame.level {
            self.frame.level = true;
            self.counts.level_frames += 1;
            if decision.tag != 0 && decision.screen == GAMEPLAY {
                self.counts.tagged += 1;
            }
        }
        decision
    }

    pub(crate) fn inputs(&mut self, ready: bool) -> Decision {
        if self.frame.level && self.frame.decision.skip == RSF_ANOMALY_SKIP_NONE && !ready {
            self.frame.decision.skip = RSF_ANOMALY_SKIP_NO_INPUTS;
        }
        self.frame.decision
    }

    pub(crate) fn presented(&mut self) {
        self.frame.presented = true;
    }

    pub(crate) fn current(&self) -> Decision {
        self.frame.decision
    }

    pub(crate) fn counters(&self) -> Counters {
        self.counts
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_history_create(scope_hold: u32) -> *mut History {
    Box::into_raw(Box::new(Mutex::new(Policy::new(scope_hold))))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_fg_history_destroy(history: *mut History) {
    drop(unsafe { Box::from_raw(history) });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_history_reset(history: &History, scope_hold: u32) {
    *history.lock().unwrap() = Policy::new(scope_hold);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_device_reset(history: &History) {
    history.lock().unwrap().device_reset();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_begin(
    history: &History,
    engine_frame: u32,
    width: u32,
    height: u32,
    cut: u32,
) -> Decision {
    history
        .lock()
        .unwrap()
        .begin(engine_frame, [width, height], cut != 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_scope(history: &History) {
    history.lock().unwrap().scope();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_ui_boundary(history: &History) -> Decision {
    history.lock().unwrap().ui_boundary()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_inputs(history: &History, ready: u32) -> Decision {
    history.lock().unwrap().inputs(ready != 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_presented(history: &History) {
    history.lock().unwrap().presented();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_end(history: &History) -> Decision {
    history.lock().unwrap().end()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_current(history: &History) -> Decision {
    history.lock().unwrap().current()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_count(history: &History, out: &mut Counters) {
    *out = history.lock().unwrap().counters();
}
