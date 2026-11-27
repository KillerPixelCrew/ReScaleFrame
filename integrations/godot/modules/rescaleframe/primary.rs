// SPDX-License-Identifier: MIT
//! Primary viewport and render target identity, render admission and SR history reset policy.
//!
//! RIDs arrive as their 64-bit identities; the C++ side keeps the typed RIDs and every Godot
//! call. Everything here except the reset request is written and read on Godot's rendering
//! server thread, as the engine module's globals were, so relaxed atomics add no ordering. The
//! reset request generation is advanced by both threads and acknowledged by the render thread.

use core::mem::{align_of, offset_of, size_of};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

use crate::{frame, pass};

// primary.h defines these driver facts.
const DRIVER_D3D12: u32 = 1;
pub(crate) const DRIVER_VULKAN: u32 = 2;

const MAX_FRAME_SECONDS: f64 = 0.25;
const MAX_CAMERA_DISTANCE: f64 = 20.0;

/// Engine facts read by the scene render hooks before admission.
#[repr(C)]
pub struct Admission {
    source_frame_id: u64,
    render_target: u64,
    host_active: u32,
    buffers_valid: u32,
    view_count: u32,
    pub(crate) driver: u32,
}

/// Engine facts read by the SR hook after its textures are known valid.
#[repr(C)]
pub struct History {
    pub(crate) color: u64,
    pub(crate) internal_width: i32,
    pub(crate) internal_height: i32,
    pub(crate) frame_seconds: f64,
    camera_distance: f64,
    sequence_break: u32,
    _reserved: u32,
}

/// The SR pass identity decided from `History`.
pub(crate) struct Identity {
    pub(crate) resource_generation: u32,
    pub(crate) reset: bool,
    pub(crate) reset_generation: u64,
}

const _: () = {
    assert!(size_of::<Admission>() == 32 && align_of::<Admission>() == 8);
    assert!(offset_of!(Admission, source_frame_id) == 0);
    assert!(offset_of!(Admission, render_target) == 8);
    assert!(offset_of!(Admission, host_active) == 16);
    assert!(offset_of!(Admission, buffers_valid) == 20);
    assert!(offset_of!(Admission, view_count) == 24);
    assert!(offset_of!(Admission, driver) == 28);
    assert!(size_of::<History>() == 40 && align_of::<History>() == 8);
    assert!(offset_of!(History, color) == 0);
    assert!(offset_of!(History, internal_width) == 8);
    assert!(offset_of!(History, internal_height) == 12);
    assert!(offset_of!(History, frame_seconds) == 16);
    assert!(offset_of!(History, camera_distance) == 24);
    assert!(offset_of!(History, sequence_break) == 32);
};

struct Primary {
    viewport: AtomicU64,
    target: AtomicU64,
    generation: AtomicU32,
    previous_color: AtomicU64,
    previous_width: AtomicI32,
    previous_height: AtomicI32,
}

static PRIMARY: Primary = Primary {
    viewport: AtomicU64::new(0),
    target: AtomicU64::new(0),
    generation: AtomicU32::new(1),
    previous_color: AtomicU64::new(0),
    previous_width: AtomicI32::new(0),
    previous_height: AtomicI32::new(0),
};

static RESET_REQUESTED: AtomicU64 = AtomicU64::new(1);
static RESET_RECORDED: AtomicU64 = AtomicU64::new(0);

const R: Ordering = Ordering::Relaxed;

fn is_primary(target: u64) -> bool {
    target == PRIMARY.target.load(R)
}

/// Rendering server thread: `configure` admitted this root viewport and its render target. A
/// new target is a new resource generation, and no SR evaluation precedes its next frame.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_primary_set(viewport: u64, target: u64) {
    PRIMARY.viewport.store(viewport, R);
    PRIMARY.target.store(target, R);
    PRIMARY.generation.fetch_add(1, R);
    frame::retarget();
    pass::invalidate();
}

/// Rendering server thread: the primary viewport's identity, zero before `configure`.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_primary_viewport() -> u64 {
    PRIMARY.viewport.load(R)
}

/// Rendering server thread: the viewport sizing hook may apply the status extent to this target.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_primary_sizing(host_active: u32, target: u64) -> u32 {
    (host_active != 0 && is_primary(target)) as u32
}

/// Any thread: the next SR pass resets its history.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_reset_request() {
    RESET_REQUESTED.fetch_add(1, Ordering::SeqCst);
}

/// Rendering server thread: a viewport attached another camera.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_camera_changed(host_active: u32, target: u64) {
    if host_active != 0 && is_primary(target) {
        rsf_godot_reset_request();
    }
}

fn admit(facts: &Admission) -> bool {
    facts.host_active != 0
        && facts.source_frame_id != 0
        && facts.buffers_valid != 0
        && is_primary(facts.render_target)
        && facts.view_count == 1
        && (facts.driver == DRIVER_D3D12 || facts.driver == DRIVER_VULKAN)
}

/// Rendering server thread: the SR hook may record a pass for this scene render.
///
/// # Safety
/// `facts` must point to a valid `rsf_godot_render_admission`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_sr_admit(facts: *const Admission) -> u32 {
    admit(unsafe { &*facts }) as u32
}

/// Rendering server thread: checks the last pass against the current target generation.
pub(crate) fn final_scene_admitted(
    facts: &Admission,
    source: u64,
    target: u64,
    generation: u32,
) -> bool {
    admit(facts)
        && source == facts.source_frame_id
        && target == facts.render_target
        && generation == PRIMARY.generation.load(R)
}

/// Rendering server thread: plans identity and history reset without changing the last pass.
pub(crate) fn decide(facts: &History) -> Identity {
    let color_changed = PRIMARY.previous_color.load(R) != facts.color;
    let reset_generation = RESET_REQUESTED.load(Ordering::SeqCst);
    let reset = reset_generation != RESET_RECORDED.load(R)
        || color_changed
        || PRIMARY.previous_width.load(R) != facts.internal_width
        || PRIMARY.previous_height.load(R) != facts.internal_height
        || facts.sequence_break != 0
        || facts.frame_seconds > MAX_FRAME_SECONDS
        || facts.camera_distance > MAX_CAMERA_DISTANCE;
    Identity {
        resource_generation: PRIMARY
            .generation
            .load(R)
            .wrapping_add(u32::from(color_changed)),
        reset,
        reset_generation,
    }
}

/// Rendering server thread: the SR pass for this frame was recorded with this color texture
/// and internal extent (`pass.rs`).
pub(crate) fn recorded(history: &History, generation: u32, reset_generation: u64) {
    PRIMARY.previous_width.store(history.internal_width, R);
    PRIMARY.previous_height.store(history.internal_height, R);
    PRIMARY.previous_color.store(history.color, R);
    PRIMARY.generation.store(generation, R);
    RESET_RECORDED.store(reset_generation, R);
    frame::evaluated();
}
