// SPDX-License-Identifier: MIT
//! Admission for sampled colour reads and the contact-light before/after pair.

use super::{observations, shader_facts};
use crate::motion_capture_records::artifacts::Bytes;
use crate::motion_capture_session::{CaptureInterval, capture_prefix, is_current};
use std::cell::Cell;
use std::ffi::{CStr, c_void};
use std::fmt::{self, Write as _};

const SHARED_READ_LIMIT: u32 = 48;
const TOTAL_READ_LIMIT: u32 = 72;
const CLOUD_COMPOSITE: u32 = 0x83524e47;

// Only the directional light with the contact ray and its AC7 noise/depth variant.
fn is_contact_light(hash: u32) -> bool {
    matches!(hash, 3619939816 | 4154163049)
}

/// The snapshot roles name the sampled images; an input without a role is recorded unsampled.
#[derive(Default)]
pub(crate) struct BeforeDrawPlan {
    pub(crate) colour_role: Option<&'static str>,
    pub(crate) loss_role: Option<&'static str>,
    pub(crate) input_roles: [Option<&'static str>; 6],
    pub(crate) input_slots: u32,
    pub(crate) sampler_slots: u32,
    pub(crate) constant_slots: u32,
    pub(crate) ps_hash: u32,
    pub(crate) lighting: bool,
}

pub(crate) fn is_cloud_composite(hash: u32) -> bool {
    hash == CLOUD_COMPOSITE
}

pub(crate) fn plan_before_draw(
    capture: CaptureInterval,
    width: u32,
    height: u32,
    pixel_shader: *mut c_void,
) -> Option<BeforeDrawPlan> {
    LIGHTING_BEFORE.set(None);
    if !capture.active() || width < 400 || height < 200 {
        return None;
    }
    let ps_hash = shader_facts(pixel_shader, 1).hash;
    let plan = if is_contact_light(ps_hash) {
        if !begin_lighting_pair(capture) {
            return None;
        }
        BeforeDrawPlan {
            colour_role: Some("contact_light_before"),
            loss_role: None,
            input_roles: [
                Some("contact_ps_input_0"),
                Some("contact_ps_input_1"),
                Some("contact_ps_input_2"),
                Some("contact_light_mask"),
                Some("contact_scene_depth"),
                Some("contact_static_shadow"),
            ],
            input_slots: 6,
            sampler_slots: 6,
            constant_slots: 2,
            ps_hash,
            lighting: true,
        }
    } else if is_cloud_composite(ps_hash) {
        BeforeDrawPlan {
            colour_role: Some("cloud_composite_before"),
            loss_role: Some("cloud_loss_before"),
            input_roles: [
                Some("cloud_near"),
                Some("cloud_far"),
                Some("cloud_near_far_depth"),
                Some("cloud_scene_depth"),
                None,
                None,
            ],
            input_slots: 4,
            sampler_slots: 0,
            constant_slots: 0,
            ps_hash,
            lighting: false,
        }
    } else {
        return None;
    };
    Some(plan)
}

#[derive(Default)]
pub(super) struct ColourReadbacks {
    attempted: u32,
    failed: u32,
    lighting_interval: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct LightingDraw {
    pub(crate) context: u64,
    pub(crate) target: u64,
    pub(crate) index: u32,
    pub(crate) hash: u32,
}

thread_local! {
    // The tap invokes both phases synchronously on the draw thread and suppresses reentry.
    static LIGHTING_BEFORE: Cell<Option<(CaptureInterval, LightingDraw)>> = const { Cell::new(None) };
}

pub(crate) fn record_lighting_before(capture: CaptureInterval, draw: LightingDraw) {
    LIGHTING_BEFORE.set(Some((capture, draw)));
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

fn begin_lighting_pair(capture: CaptureInterval) -> bool {
    let mut observations = observations();
    let colour = &mut observations.colour;
    if !capture.active()
        || CaptureInterval::load() != capture
        || colour.lighting_interval == Some(capture.interval)
    {
        return false;
    }
    colour.lighting_interval = Some(capture.interval);
    true
}

pub(crate) fn take_lighting_after(capture: CaptureInterval, draw: &LightingDraw) -> bool {
    let before = LIGHTING_BEFORE.take();
    capture.active() && CaptureInterval::load() == capture && before == Some((capture, *draw))
}

pub(crate) fn colour_snapshot_available(capture: CaptureInterval, lighting: bool) -> bool {
    observations().colour.available(capture, lighting)
}

fn reserve_colour_snapshot(capture: CaptureInterval, lighting: bool) -> Option<u32> {
    let mut observations = observations();
    let colour = &mut observations.colour;
    if !colour.available(capture, lighting) {
        return None;
    }
    let ordinal = colour.attempted;
    colour.attempted += 1;
    Some(ordinal)
}

pub(crate) struct ColourSnapshot {
    pub(crate) name: [u8; 96],
    pub(crate) result: i32,
}

impl Default for ColourSnapshot {
    fn default() -> Self {
        Self {
            name: [0; 96],
            result: 0,
        }
    }
}

/// Reserves a snapshot of the sample's `role` image and names it in `snapshot`; `read` dumps it
/// to the given narrow path prefix and returns the dump result. False on a formatting failure.
pub(crate) fn colour_snapshot(
    capture: CaptureInterval,
    role: &str,
    lighting: bool,
    read: impl FnOnce(&CStr) -> i32,
    snapshot: &mut ColourSnapshot,
) -> bool {
    let Some(prefix) = capture_prefix(capture.session) else {
        return true;
    };
    let Some(ordinal) = reserve_colour_snapshot(capture, lighting) else {
        return true;
    };
    let result = (|| -> fmt::Result {
        use std::io::Write as _;
        let mut name = &mut snapshot.name[..];
        let initial = name.len();
        write!(name, "f{:03}_stage_{role}_{ordinal:03}", capture.interval)
            .map_err(|_| fmt::Error)?;
        let name_len = initial - name.len();
        let mut path = Bytes::default();
        path.extend(prefix.to_bytes())?;
        path.write_char('\\')?;
        path.extend(&snapshot.name[..name_len])?;
        path.write_char('\0')?;
        snapshot.result = read(CStr::from_bytes_with_nul(&path.0).map_err(|_| fmt::Error)?);
        if snapshot.result != 0 {
            let mut observations = observations();
            if is_current(capture.session) {
                observations.colour.failed += 1;
            }
        }
        Ok(())
    })();
    result.is_ok()
}
