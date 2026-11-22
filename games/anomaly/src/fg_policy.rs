// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_SCREEN_FLIGHT, RSF_SCREEN_UNKNOWN};

pub(crate) const RSF_ANOMALY_SKIP_NONE: i32 = 0;
const RSF_ANOMALY_SKIP_NO_LEVEL: i32 = 1;
pub(crate) const RSF_ANOMALY_SKIP_SCOPE: i32 = 2;
pub(crate) const RSF_ANOMALY_SKIP_NO_PRESENT: i32 = 3;
const RSF_ANOMALY_SKIP_NO_INPUTS: i32 = 4;
const RSF_ANOMALY_SKIP_COUNT: usize = 5;
pub(crate) const RSF_ANOMALY_RESET_NONE: i32 = 0;
const RSF_ANOMALY_RESET_FIRST: i32 = 1;
pub(crate) const RSF_ANOMALY_RESET_DEVICE: i32 = 2;
pub(crate) const RSF_ANOMALY_RESET_DISCONTINUITY: i32 = 3;
pub(crate) const RSF_ANOMALY_RESET_SCOPE_END: i32 = 4;
const RSF_ANOMALY_RESET_SIZE: i32 = 5;
const RSF_ANOMALY_RESET_CUT: i32 = 6;
const RSF_ANOMALY_RESET_COUNT: usize = 7;

#[derive(Default, Copy, Clone)]
pub(crate) struct Decision {
    pub tag: u32,
    pub screen: u32,
    pub reset: i32,
    pub skip: i32,
}

#[derive(Default, Copy, Clone)]
pub(crate) struct Counters {
    pub source_frames: u64,
    pub level_frames: u64,
    pub eligible_at_ui_boundary: u64,
    pub fed: u64,
    pub presented: u64,
    pub unpresented: u64,
    pub scope_frames: u64,
    pub skipped: [u64; RSF_ANOMALY_SKIP_COUNT],
    pub resets: [u64; RSF_ANOMALY_RESET_COUNT],
}

const GAMEPLAY: u32 = RSF_SCREEN_FLIGHT;
const HELD: u32 = RSF_SCREEN_UNKNOWN;

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
                self.counts.eligible_at_ui_boundary += 1;
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

    #[cfg(windows)]
    pub(crate) fn current(&self) -> Decision {
        self.frame.decision
    }

    pub(crate) fn counters(&self) -> Counters {
        self.counts
    }
}

#[cfg(test)]
mod tests;
