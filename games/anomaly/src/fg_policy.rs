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

struct Policy {
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
    fn new(scope_hold: u32) -> Self {
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

    fn begin(&mut self, engine_frame: u32, extent: [u32; 2], cut: bool) -> Decision {
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

    fn end(&mut self) -> Decision {
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
    let mut policy = history.lock().unwrap();
    if policy.pending == RSF_ANOMALY_RESET_NONE {
        policy.pending = RSF_ANOMALY_RESET_DEVICE;
    }
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
    let mut policy = history.lock().unwrap();
    if !policy.frame.scope {
        policy.counts.scope_frames += 1;
    }
    policy.frame.scope = true;
    policy.last_scope = Some(policy.engine_frame);
    policy.frame.decision.skip = RSF_ANOMALY_SKIP_SCOPE;
    policy.frame.decision.screen = HELD;
    policy.frame.decision.tag = 0;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_ui_boundary(history: &History) -> Decision {
    let mut policy = history.lock().unwrap();
    let decision = policy.frame.decision;
    if !policy.frame.level {
        policy.frame.level = true;
        policy.counts.level_frames += 1;
        if decision.tag != 0 && decision.screen == GAMEPLAY {
            policy.counts.tagged += 1;
        }
    }
    decision
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_inputs(history: &History, ready: u32) -> Decision {
    let mut policy = history.lock().unwrap();
    if policy.frame.level && policy.frame.decision.skip == RSF_ANOMALY_SKIP_NONE && ready == 0 {
        policy.frame.decision.skip = RSF_ANOMALY_SKIP_NO_INPUTS;
    }
    policy.frame.decision
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_presented(history: &History) {
    history.lock().unwrap().frame.presented = true;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_end(history: &History) -> Decision {
    history.lock().unwrap().end()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_current(history: &History) -> Decision {
    history.lock().unwrap().frame.decision
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_count(history: &History, out: &mut Counters) {
    *out = history.lock().unwrap().counts;
}
