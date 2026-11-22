// SPDX-License-Identifier: MIT
//! Admission for sampled colour reads and the contact-light before/after pair.

use super::{observations, rsf_ac7_capture_shader_facts};
use crate::motion_capture_records::artifacts::Bytes;
use crate::motion_capture_session::{CaptureInterval, capture_prefix, rsf_ac7_capture_current};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::fmt::{self, Write as _};
use std::mem::{offset_of, size_of};

const SHARED_READ_LIMIT: u32 = 48;
const TOTAL_READ_LIMIT: u32 = 72;
const CLOUD_COMPOSITE: u32 = 0x83524e47;

// Only the directional light with the contact ray and its AC7 noise/depth variant.
fn is_contact_light(hash: u32) -> bool {
    matches!(hash, 3619939816 | 4154163049)
}

#[repr(C)]
pub(crate) struct BeforeDrawPlan {
    colour_role: *const c_char,
    loss_role: *const c_char,
    input_roles: [*const c_char; 6],
    input_slots: u32,
    sampler_slots: u32,
    constant_slots: u32,
    pub(crate) ps_hash: u32,
    pub(crate) lighting: u32,
}

const _: () = {
    assert!(size_of::<BeforeDrawPlan>() == 88 && offset_of!(BeforeDrawPlan, input_slots) == 64);
    assert!(offset_of!(BeforeDrawPlan, ps_hash) == 76);
};

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_is_cloud_composite(hash: u32) -> bool {
    hash == CLOUD_COMPOSITE
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_plan_before_draw(
    capture: CaptureInterval,
    width: u32,
    height: u32,
    pixel_shader: *mut c_void,
    plan: &mut BeforeDrawPlan,
) -> bool {
    LIGHTING_BEFORE.set(None);
    if !capture.active() || width < 400 || height < 200 {
        return false;
    }
    let ps_hash = rsf_ac7_capture_shader_facts(pixel_shader, 1).hash;
    *plan = if is_contact_light(ps_hash) {
        if !begin_lighting_pair(capture) {
            return false;
        }
        BeforeDrawPlan {
            colour_role: c"contact_light_before".as_ptr(),
            loss_role: std::ptr::null(),
            input_roles: [
                c"contact_ps_input_0".as_ptr(),
                c"contact_ps_input_1".as_ptr(),
                c"contact_ps_input_2".as_ptr(),
                c"contact_light_mask".as_ptr(),
                c"contact_scene_depth".as_ptr(),
                c"contact_static_shadow".as_ptr(),
            ],
            input_slots: 6,
            sampler_slots: 6,
            constant_slots: 2,
            ps_hash,
            lighting: 1,
        }
    } else if rsf_ac7_capture_is_cloud_composite(ps_hash) {
        BeforeDrawPlan {
            colour_role: c"cloud_composite_before".as_ptr(),
            loss_role: c"cloud_loss_before".as_ptr(),
            input_roles: [
                c"cloud_near".as_ptr(),
                c"cloud_far".as_ptr(),
                c"cloud_near_far_depth".as_ptr(),
                c"cloud_scene_depth".as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            ],
            input_slots: 4,
            sampler_slots: 0,
            constant_slots: 0,
            ps_hash,
            lighting: 0,
        }
    } else {
        return false;
    };
    true
}

#[derive(Default)]
pub(super) struct ColourReadbacks {
    attempted: u32,
    failed: u32,
    lighting_interval: Option<u32>,
}

#[repr(C)]
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

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_take_lighting_after(
    capture: CaptureInterval,
    draw: &LightingDraw,
) -> bool {
    let before = LIGHTING_BEFORE.take();
    capture.active() && CaptureInterval::load() == capture && before == Some((capture, *draw))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_colour_snapshot_available(
    capture: CaptureInterval,
    lighting: bool,
) -> bool {
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

#[repr(C)]
pub(crate) struct ColourSnapshot {
    pub(crate) name: [u8; 96],
    pub(crate) result: i32,
}

type ReadColour = unsafe extern "C" fn(*mut c_void, *const c_char) -> i32;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_colour_snapshot(
    capture: CaptureInterval,
    role: *const c_char,
    lighting: bool,
    user: *mut c_void,
    read: ReadColour,
    snapshot: &mut ColourSnapshot,
) -> bool {
    let Some(prefix) = capture_prefix(capture.session) else {
        return true;
    };
    let Some(ordinal) = reserve_colour_snapshot(capture, lighting) else {
        return true;
    };
    // SAFETY: The native capture supplies a static terminated role string.
    let Ok(role) = unsafe { CStr::from_ptr(role) }.to_str() else {
        return false;
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
        // SAFETY: The native leaf borrows the path and its original COM state for this call.
        snapshot.result = unsafe { read(user, path.0.as_ptr().cast()) };
        if snapshot.result != 0 {
            let mut observations = observations();
            if rsf_ac7_capture_current(capture.session) {
                observations.colour.failed += 1;
            }
        }
        Ok(())
    })();
    result.is_ok()
}
