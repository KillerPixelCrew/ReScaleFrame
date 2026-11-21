// SPDX-License-Identifier: MIT
//! Godot CPU frame lifecycle, queue handoff and render frame identity.
//!
//! The main loop callbacks own the CPU state and the render thread callbacks own the render
//! state, as the engine module's globals did. Relaxed atomics keep the cross-thread input read
//! defined without adding ordering.

use core::mem::{align_of, offset_of, size_of};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// Matches game_renderer.h. frame.h checks these values.
const FRAME_BEGIN: u32 = 0;
const INPUT_SAMPLE: u32 = 1;
const SIMULATION_BEGIN: u32 = 2;
const SIMULATION_END: u32 = 3;
const FRAME_END: u32 = 4;
const INPUT_EVENT: u32 = 5;

const DEFAULT_SECONDS: f64 = 1.0 / 60.0;

/// Native typed engine callbacks. The C++ side reads the clock, reports records to the DLL
/// and reads Godot's pause and focus state for the render thread handoff.
#[repr(C)]
pub struct FrameCallbacks {
    now_usec: unsafe extern "C" fn() -> u64,
    report: unsafe extern "C" fn(
        stage: u32,
        source_frame_id: u64,
        render_expected: u32,
        input_kind: u32,
        unframed_input: u32,
    ),
    queue_render: unsafe extern "C" fn(source_frame_id: u64, cpu_seconds: f64),
}

/// Render thread view of the frame most recently queued by the main loop.
#[repr(C)]
pub struct RenderFrame {
    source_frame_id: u64,
    frame_seconds: f64,
    gameplay: u32,
    sequence_break: u32,
}

const _: () = {
    assert!(size_of::<RenderFrame>() == 24);
    assert!(align_of::<RenderFrame>() == 8);
    assert!(offset_of!(RenderFrame, source_frame_id) == 0);
    assert!(offset_of!(RenderFrame, frame_seconds) == 8);
    assert!(offset_of!(RenderFrame, gameplay) == 16);
    assert!(offset_of!(RenderFrame, sequence_break) == 20);
};

struct Cpu {
    open: AtomicBool,
    simulation_ended: AtomicBool,
    queued: AtomicBool,
    frame: AtomicU64,
    previous_usec: AtomicU64,
    seconds: AtomicU64,
}

struct Render {
    frame: AtomicU64,
    gameplay: AtomicBool,
    seconds: AtomicU64,
    last_evaluated: AtomicU64,
}

static CPU: Cpu = Cpu {
    open: AtomicBool::new(false),
    simulation_ended: AtomicBool::new(false),
    queued: AtomicBool::new(false),
    frame: AtomicU64::new(0),
    previous_usec: AtomicU64::new(0),
    seconds: AtomicU64::new(DEFAULT_SECONDS.to_bits()),
};

static RENDER: Render = Render {
    frame: AtomicU64::new(0),
    gameplay: AtomicBool::new(true),
    seconds: AtomicU64::new(DEFAULT_SECONDS.to_bits()),
    last_evaluated: AtomicU64::new(0),
};

const R: Ordering = Ordering::Relaxed;

/// Reports a lifecycle stage for the current CPU frame. No frame has been opened while the
/// identity is zero, so nothing is reported.
fn emit(callbacks: &FrameCallbacks, stage: u32, render_expected: u32) {
    let frame = CPU.frame.load(R);
    if frame != 0 {
        // SAFETY: frame.h requires valid callbacks.
        unsafe { (callbacks.report)(stage, frame, render_expected, 0, 0) };
    }
}

/// # Safety
/// `callbacks` must point to valid callbacks. Call on the main loop thread after the native
/// host is ready and enabled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_cpu_begin(callbacks: *const FrameCallbacks) {
    let callbacks = unsafe { &*callbacks };
    if CPU.open.load(R) {
        return;
    }
    // SAFETY: valid callbacks, see above.
    let now = unsafe { (callbacks.now_usec)() };
    let previous = CPU.previous_usec.load(R);
    let seconds = if previous != 0 {
        now.wrapping_sub(previous) as f64 / 1_000_000.0
    } else {
        DEFAULT_SECONDS
    };
    CPU.seconds.store(seconds.to_bits(), R);
    CPU.previous_usec.store(now, R);
    CPU.frame.store(CPU.frame.load(R).wrapping_add(1), R);
    CPU.open.store(true, R);
    CPU.simulation_ended.store(false, R);
    CPU.queued.store(false, R);
    emit(callbacks, FRAME_BEGIN, 0);
    emit(callbacks, INPUT_SAMPLE, 0);
}

/// # Safety
/// `callbacks` must point to valid callbacks. Call on the main loop thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_cpu_simulation_begin(callbacks: *const FrameCallbacks) {
    let callbacks = unsafe { &*callbacks };
    if CPU.open.load(R) {
        emit(callbacks, SIMULATION_BEGIN, 0);
    }
}

/// Ends simulation before Godot's draw and hands the frame identity to the render thread. A
/// draw that will not present queues identity zero, which the render callbacks refuse.
///
/// # Safety
/// `callbacks` must point to valid callbacks. Call on the main loop thread after the native
/// host is ready and enabled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_cpu_queue(callbacks: *const FrameCallbacks, present: u32) {
    let callbacks = unsafe { &*callbacks };
    emit(callbacks, SIMULATION_END, 0);
    CPU.simulation_ended.store(true, R);
    CPU.queued.store(present != 0, R);
    let frame = if present != 0 { CPU.frame.load(R) } else { 0 };
    // SAFETY: valid callbacks, see above.
    unsafe { (callbacks.queue_render)(frame, f64::from_bits(CPU.seconds.load(R))) };
}

/// # Safety
/// `callbacks` must point to valid callbacks. Call on the main loop thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_cpu_end(callbacks: *const FrameCallbacks) {
    let callbacks = unsafe { &*callbacks };
    if !CPU.simulation_ended.load(R) {
        emit(callbacks, SIMULATION_END, 0);
    }
    emit(callbacks, FRAME_END, CPU.queued.load(R) as u32);
    CPU.open.store(false, R);
}

/// Attributes an admitted input kind to the open CPU frame, or reports it unframed.
///
/// # Safety
/// `callbacks` must point to valid callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_cpu_input(callbacks: *const FrameCallbacks, kind: u32) {
    let callbacks = unsafe { &*callbacks };
    let framed = CPU.open.load(R);
    let source = if framed { CPU.frame.load(R) } else { 0 };
    // SAFETY: valid callbacks, see above.
    unsafe { (callbacks.report)(INPUT_EVENT, source, 0, kind, (!framed) as u32) };
}

/// Render thread: the queued draw begins.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_render_frame_begin(source_frame_id: u64, gameplay: u32, seconds: f64) {
    RENDER.frame.store(source_frame_id, R);
    RENDER.gameplay.store(gameplay != 0, R);
    RENDER.seconds.store(seconds.to_bits(), R);
}

/// Render thread: reads the current frame. A sequence break means the previous SR evaluation was
/// not the immediately preceding CPU frame, or the primary target changed since it.
///
/// # Safety
/// `out` must point to writable, aligned storage for one `rsf_godot_render_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_render_frame_read(out: *mut RenderFrame) {
    let frame = RENDER.frame.load(R);
    let value = RenderFrame {
        source_frame_id: frame,
        frame_seconds: f64::from_bits(RENDER.seconds.load(R)),
        gameplay: RENDER.gameplay.load(R) as u32,
        sequence_break: (RENDER.last_evaluated.load(R).wrapping_add(1) != frame) as u32,
    };
    unsafe { out.write(value) };
}

/// Render thread: the current frame's SR pass was recorded.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_render_frame_evaluated() {
    RENDER.last_evaluated.store(RENDER.frame.load(R), R);
}

/// Render thread: the primary render target changed, so no evaluation precedes the next frame.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_render_frame_retarget() {
    RENDER.last_evaluated.store(0, R);
}
