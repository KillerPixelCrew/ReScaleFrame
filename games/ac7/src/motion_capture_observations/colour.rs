// SPDX-License-Identifier: MIT
//! Admission for sampled colour reads and the contact-light before/after pair.

use super::observations;
use crate::motion_capture_session::{CaptureInterval, rsf_ac7_capture_current};

const SHARED_READ_LIMIT: u32 = 48;
const TOTAL_READ_LIMIT: u32 = 72;

#[derive(Default)]
pub(super) struct ColourReadbacks {
    attempted: u32,
    failed: u32,
    lighting_interval: Option<u32>,
    lighting_after_pending: bool,
}

impl ColourReadbacks {
    pub(super) fn counts(&self) -> (u32, u32) {
        (self.attempted, self.failed)
    }

    fn available(&self, capture: CaptureInterval, lighting: bool) -> bool {
        let limit = if lighting {
            TOTAL_READ_LIMIT
        } else {
            SHARED_READ_LIMIT
        };
        capture.active() && CaptureInterval::load() == capture && self.attempted < limit
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_begin_lighting_pair(capture: CaptureInterval) -> bool {
    let mut observations = observations();
    let colour = &mut observations.colour;
    if !capture.active()
        || CaptureInterval::load() != capture
        || colour.lighting_interval == Some(capture.interval)
    {
        return false;
    }
    colour.lighting_interval = Some(capture.interval);
    colour.lighting_after_pending = true;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_take_lighting_after(capture: CaptureInterval) -> bool {
    let mut observations = observations();
    let colour = &mut observations.colour;
    capture.active()
        && CaptureInterval::load() == capture
        && colour.lighting_interval == Some(capture.interval)
        && std::mem::take(&mut colour.lighting_after_pending)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_colour_snapshot_available(
    capture: CaptureInterval,
    lighting: bool,
) -> bool {
    observations().colour.available(capture, lighting)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_reserve_colour_snapshot(
    capture: CaptureInterval,
    lighting: bool,
) -> u32 {
    let mut observations = observations();
    let colour = &mut observations.colour;
    if !colour.available(capture, lighting) {
        return u32::MAX;
    }
    let ordinal = colour.attempted;
    colour.attempted += 1;
    ordinal
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_colour_snapshot_failed(capture: CaptureInterval) {
    let mut observations = observations();
    if rsf_ac7_capture_current(capture.session) {
        observations.colour.failed += 1;
    }
}
