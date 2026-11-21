// SPDX-License-Identifier: MIT
//! Primary viewport and render target identity, render admission and SR history reset policy.
//!
//! RIDs arrive as their 64-bit identities; the C++ side keeps the typed RIDs and every Godot
//! call. Everything here except the reset request is written and read on Godot's rendering
//! server thread, as the engine module's globals were, so relaxed atomics add no ordering. The
//! reset request is set from the main and rendering server threads and keeps the sequentially
//! consistent exchange of the `std::atomic<bool>` it replaces.

use core::mem::{align_of, offset_of, size_of};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use crate::frame;

// primary.h defines these driver facts.
const DRIVER_D3D12: u32 = 1;
const DRIVER_VULKAN: u32 = 2;

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
    driver: u32,
}

/// Engine facts read by the SR hook after its textures are known valid.
#[repr(C)]
pub struct History {
    color: u64,
    internal_width: i32,
    internal_height: i32,
    frame_seconds: f64,
    camera_distance: f64,
    sequence_break: u32,
    reserved: u32,
}

/// The SR pass identity decided from `History`.
#[repr(C)]
pub struct Identity {
    resource_generation: u32,
    reset: u32,
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
    assert!(size_of::<Identity>() == 8 && align_of::<Identity>() == 4);
    assert!(offset_of!(Identity, resource_generation) == 0);
    assert!(offset_of!(Identity, reset) == 4);
};

struct Primary {
    viewport: AtomicU64,
    target: AtomicU64,
    generation: AtomicU32,
    previous_color: AtomicU64,
    previous_width: AtomicI32,
    previous_height: AtomicI32,
    recorded_source: AtomicU64,
}

static PRIMARY: Primary = Primary {
    viewport: AtomicU64::new(0),
    target: AtomicU64::new(0),
    generation: AtomicU32::new(1),
    previous_color: AtomicU64::new(0),
    previous_width: AtomicI32::new(0),
    previous_height: AtomicI32::new(0),
    recorded_source: AtomicU64::new(0),
};

static RESET_REQUESTED: AtomicBool = AtomicBool::new(true);

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
    RESET_REQUESTED.store(true, Ordering::SeqCst);
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

/// Rendering server thread: the final-scene hook may record a pass. It also requires the last
/// recorded SR pass to carry this frame's identity.
///
/// # Safety
/// `facts` must point to a valid `rsf_godot_render_admission`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_final_scene_admit(facts: *const Admission) -> u32 {
    let facts = unsafe { &*facts };
    (admit(facts) && PRIMARY.recorded_source.load(R) == facts.source_frame_id) as u32
}

/// Rendering server thread: decides the SR pass generation and history reset. A new internal
/// color texture is a new generation even if the pass is later refused. The reset request is
/// consumed first, so it is consumed by every pass that reaches this point.
///
/// # Safety
/// `facts` must point to a valid `rsf_godot_sr_history` and `out` to writable, aligned storage
/// for one `rsf_godot_sr_identity`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_sr_decide(facts: *const History, out: *mut Identity) {
    let facts = unsafe { &*facts };
    let color_changed = PRIMARY.previous_color.load(R) != facts.color;
    if color_changed {
        PRIMARY.generation.fetch_add(1, R);
    }
    let reset = RESET_REQUESTED.swap(false, Ordering::SeqCst)
        || color_changed
        || PRIMARY.previous_width.load(R) != facts.internal_width
        || PRIMARY.previous_height.load(R) != facts.internal_height
        || facts.sequence_break != 0
        || facts.frame_seconds > MAX_FRAME_SECONDS
        || facts.camera_distance > MAX_CAMERA_DISTANCE;
    let value = Identity {
        resource_generation: PRIMARY.generation.load(R),
        reset: reset as u32,
    };
    unsafe { out.write(value) };
}

/// Rendering server thread: the SR pass for this frame was recorded with this color texture
/// and internal extent.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_sr_recorded(
    source_frame_id: u64,
    color: u64,
    internal_width: i32,
    internal_height: i32,
) {
    PRIMARY.previous_width.store(internal_width, R);
    PRIMARY.previous_height.store(internal_height, R);
    PRIMARY.previous_color.store(color, R);
    frame::evaluated();
    PRIMARY.recorded_source.store(source_frame_id, R);
}
