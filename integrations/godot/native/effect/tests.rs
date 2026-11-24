// SPDX-License-Identifier: MIT
//! The effect owner against recording leaves. The godot-cpp reads and the readback compute pass
//! are not exercised here; the Godot demo (`demo/main.gd`) covers them in a running engine.
use super::*;
use std::cell::{Cell, RefCell};

thread_local! {
    static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    /// `None` reports no RenderingDevice, scene buffers or scene data.
    static FRAME: Cell<Option<EffectFrame>> = const { Cell::new(None) };
    static RESOURCES: Cell<EffectResources> = Cell::new(resources(EFFECT_MOTION_FORMAT));
    static BYTES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

const CALL: *mut c_void = 0x40 as *mut c_void;

pub(super) unsafe fn leaf_frame(call: *mut c_void, frame: *mut EffectFrame) -> u32 {
    assert_eq!(call, CALL);
    CALLS.with_borrow_mut(|calls| calls.push("frame"));
    match FRAME.get() {
        Some(value) => {
            // SAFETY: the owner's record.
            unsafe { frame.write(value) };
            1
        }
        None => 0,
    }
}

pub(super) unsafe fn leaf_resources(call: *mut c_void, resources: *mut EffectResources) -> u32 {
    assert_eq!(call, CALL);
    CALLS.with_borrow_mut(|calls| calls.push("resources"));
    // SAFETY: the owner's record.
    unsafe { resources.write(RESOURCES.get()) };
    1
}

pub(super) unsafe fn leaf_read_motion(
    call: *mut c_void,
    plan: *const EffectReadback,
    data: *mut *const u8,
    size: *mut u64,
) -> u32 {
    assert_eq!(call, CALL);
    // SAFETY: the owner lends the plan for this call.
    let plan = unsafe { &*plan };
    let frame = FRAME.get().expect("motion readback requires a frame");
    assert_eq!(u64::from(plan.bytes), pixels(&frame) * SAMPLE_BYTES as u64);
    assert_eq!(
        plan.groups_x,
        frame.internal_width.unsigned_abs().div_ceil(8)
    );
    assert_eq!(
        plan.groups_y,
        frame.internal_height.unsigned_abs().div_ceil(8)
    );
    CALLS.with_borrow_mut(|calls| calls.push("read_motion"));
    BYTES.with_borrow(|bytes| {
        // SAFETY: the owner's outputs; the bytes are not touched until the call returns.
        unsafe {
            data.write(bytes.as_ptr());
            size.write(bytes.len() as u64);
        }
    });
    1
}

fn resources(motion_format: i32) -> EffectResources {
    EffectResources {
        color_format: 97,
        depth_format: 125,
        motion_format,
        native_device: 1,
        native_queue: 1,
        native_textures: 1,
    }
}

fn admitted() -> EffectFrame {
    EffectFrame {
        render_target: 0x7000_0001,
        views: 1,
        internal_width: 3,
        internal_height: 2,
        output_width: 6,
        output_height: 4,
        color_valid: 1,
        depth_valid: 1,
        motion_valid: 1,
        ..EffectFrame::default()
    }
}

fn samples(values: &[(f32, f32)]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|&(x, y)| x.to_le_bytes().into_iter().chain(y.to_le_bytes()))
        .collect()
}

fn reset(frame: Option<EffectFrame>, bytes: Vec<u8>) {
    CALLS.with_borrow_mut(Vec::clear);
    FRAME.set(frame);
    RESOURCES.set(resources(EFFECT_MOTION_FORMAT));
    BYTES.set(bytes);
}

fn calls() -> Vec<&'static str> {
    CALLS.with_borrow_mut(std::mem::take)
}

fn status(effect: &Effect) -> EffectStatus {
    effect.status()
}

const SIX: [(f32, f32); 6] = [
    (0.0, 0.0),
    (-1.0, -1.0),
    (f32::NAN, 0.0),
    (0.25, -0.5),
    (0.00001, 0.0),
    (-1.0, -0.5),
];

#[test]
fn classification_counts_each_texel_once() {
    let capture = classify(&samples(&SIX), 6);
    assert!(capture.succeeded);
    assert_eq!(capture.pixels, 6);
    assert_eq!(capture.nonfinite_pixels, 1);
    assert_eq!(capture.unwritten_pixels, 1);
    // 0.25/-0.5 and -1/-0.5 move. The f32 nearest 0.00001 widens below the f64 threshold, so it
    // is still, as Godot's decode_float compared in double.
    assert_eq!(capture.moving_pixels, 2);
    assert_eq!(capture.max_abs_uv, 1.0);
}

#[test]
fn classification_keeps_valid_zero_and_infinity_apart() {
    let capture = classify(
        &samples(&[(0.0, -0.0), (f32::INFINITY, 0.0), (-2.0, -1.0), (-1.0, 0.0)]),
        4,
    );
    assert_eq!(capture.nonfinite_pixels, 1);
    assert_eq!(capture.unwritten_pixels, 1);
    assert_eq!(capture.moving_pixels, 1);
    assert_eq!(capture.max_abs_uv, 1.0);
}

#[test]
fn a_readback_of_the_wrong_length_fails_and_counts_nothing() {
    for bytes in [Vec::new(), samples(&SIX[..5]), samples(&SIX)[..47].to_vec()] {
        let capture = classify(&bytes, 6);
        assert_eq!(
            capture,
            Capture {
                pixels: 6,
                ..Capture::default()
            }
        );
    }
}

#[test]
fn buffer_admission_needs_one_view_an_extent_and_three_textures() {
    assert!(buffers_admitted(&admitted()));
    let refused = [
        EffectFrame {
            views: 2,
            ..admitted()
        },
        EffectFrame {
            internal_width: 0,
            ..admitted()
        },
        EffectFrame {
            internal_height: -1,
            ..admitted()
        },
        EffectFrame {
            color_valid: 0,
            ..admitted()
        },
        EffectFrame {
            depth_valid: 0,
            ..admitted()
        },
        EffectFrame {
            motion_valid: 0,
            ..admitted()
        },
    ];
    for frame in refused {
        assert!(!buffers_admitted(&frame), "{frame:?}");
    }
}

#[test]
fn a_requested_capture_reads_and_publishes_with_the_snapshot() {
    reset(Some(admitted()), samples(&SIX));
    let effect = Effect::default();
    effect.render(CALL);
    assert_eq!(calls(), ["frame", "resources"]);
    effect.request_capture();
    effect.render(CALL);
    assert_eq!(calls(), ["frame", "resources", "read_motion"]);
    let value = status(&effect);
    assert_eq!(value.callbacks, 2);
    assert_eq!(value.frame, admitted());
    assert_eq!(value.resources, resources(EFFECT_MOTION_FORMAT));
    assert_eq!(value.buffers_available, 1);
    assert_eq!(value.captures, 1);
    assert_eq!(value.capture_callback, 2);
    assert_eq!(value.capture_succeeded, 1);
    assert_eq!(value.capture_pixels, 6);
    assert_eq!(value.capture_moving_pixels, 2);
    assert_eq!(value.capture_unwritten_pixels, 1);
    assert_eq!(value.capture_nonfinite_pixels, 1);
    // The request was taken: later callbacks keep the capture and read nothing.
    effect.render(CALL);
    assert_eq!(calls(), ["frame", "resources"]);
    let later = status(&effect);
    assert_eq!(later.callbacks, 3);
    assert_eq!((later.captures, later.capture_callback), (1, 2));
}

#[test]
fn a_request_without_readable_motion_is_answered_unsucceeded() {
    let effect = Effect::default();
    // Another velocity format: no readback.
    reset(Some(admitted()), samples(&SIX));
    RESOURCES.set(resources(102));
    effect.request_capture();
    effect.render(CALL);
    assert_eq!(calls(), ["frame", "resources"]);
    let value = status(&effect);
    assert_eq!((value.captures, value.capture_callback), (1, 1));
    assert_eq!((value.capture_succeeded, value.capture_pixels), (0, 0));
    assert_eq!(value.resources.motion_format, 102);
    // Buffers not admitted: no format or handle read, defaults published.
    reset(
        Some(EffectFrame {
            views: 2,
            ..admitted()
        }),
        samples(&SIX),
    );
    effect.request_capture();
    effect.render(CALL);
    assert_eq!(calls(), ["frame"]);
    let value = status(&effect);
    assert_eq!(value.buffers_available, 0);
    assert_eq!(value.resources, EffectResources::default());
    assert_eq!(value.frame.views, 2);
    assert_eq!((value.captures, value.capture_callback), (2, 2));
    // No scene buffers: the default snapshot, still counted.
    reset(None, samples(&SIX));
    effect.request_capture();
    effect.render(CALL);
    assert_eq!(calls(), ["frame"]);
    let value = status(&effect);
    assert_eq!(value.callbacks, 3);
    assert_eq!(value.frame, EffectFrame::default());
    assert_eq!((value.captures, value.capture_callback), (3, 3));
    // A refused readback (empty bytes) at the admitted extent.
    reset(Some(admitted()), Vec::new());
    effect.request_capture();
    effect.render(CALL);
    assert_eq!(calls(), ["frame", "resources", "read_motion"]);
    let value = status(&effect);
    assert_eq!(
        (
            value.captures,
            value.capture_succeeded,
            value.capture_pixels
        ),
        (4, 0, 6)
    );
}

#[test]
fn a_new_effect_publishes_the_probe_defaults() {
    let value = status(&Effect::default());
    assert_eq!(value.callbacks, 0);
    assert_eq!(value.frame, EffectFrame::default());
    assert_eq!(value.frame.camera_transform[0], 1.0);
    assert_eq!(value.frame.camera_transform[4], 1.0);
    assert_eq!(value.frame.camera_transform[8], 1.0);
    assert_eq!(value.frame.camera_transform[9..], [0.0; 3]);
    assert_eq!(value.frame.jittered_projection[15], 1.0);
    assert_eq!(value.resources.color_format, -1);
    assert_eq!(value.capture_max_abs_uv, 0.0);
    assert_eq!(
        (
            value.rendering_ready,
            value.sr_active,
            value.fg_active,
            value.latency_active
        ),
        (0, 0, 0, 0)
    );
    // SAFETY: owner-held static strings.
    let text = |value: *const std::ffi::c_char| unsafe { CStr::from_ptr(value) };
    assert_eq!(text(value.stage), c"renderer_probe");
    assert_eq!(
        text(value.snapshot_scope),
        c"last_post_transparent_callback"
    );
    assert_eq!(text(value.reason), REASON);
}
