// SPDX-License-Identifier: MIT
//! CPU scenarios for the independent temporal model.
mod geometry_bridge;
mod history;
mod math;
mod oracle;

use crate::sys::{
    rsf_anomaly_plan_camera as Camera, rsf_anomaly_plan_extent as Extent,
    rsf_anomaly_plan_frame as Frame, rsf_anomaly_plan_key as Key,
    rsf_anomaly_plan_matrix as Matrix, rsf_anomaly_plan_resolutions as Sizes,
    rsf_anomaly_plan_token as Token, rsf_anomaly_plan_vec2 as Vec2, rsf_anomaly_plan_vec4 as Vec4,
};
use oracle::{Capture, History, Pose};
use std::ffi::{CStr, c_char};
use std::panic::catch_unwind;

const MAIN: u8 = 0;
const SCOPE: u8 = 1;
const WORLD: u8 = 0;
const FIRST_PERSON: u8 = 1;
const STARTED: u32 = 0;
const REPEATED: u32 = 1;
const INVALID: u32 = 2;
const OUT_OF_ORDER: u32 = 3;
const FRAME_CONFLICT: u32 = 4;
const CAPTURED: u32 = 0;
const CAPTURE_REPEATED: u32 = 1;
const CAPTURE_INVALID: u32 = 2;
const STALE: u32 = 3;
const CAPTURE_CONFLICT: u32 = 4;
const RETIRED: u32 = 5;
const MISSING_PROJECTION: u32 = 6;
const SKINNED: u8 = 1;
const DEFORMED: u8 = 2;
const DETAIL: u8 = 3;

fn identity() -> Matrix {
    Matrix {
        values: std::array::from_fn(|i| f64::from(u8::from(i % 5 == 0))),
    }
}

fn sizes() -> Sizes {
    Sizes {
        output: extent(1920, 1080),
        render: extent(1280, 720),
        pda: extent(1024, 512),
    }
}

fn extent(width: u32, height: u32) -> Extent {
    Extent { width, height }
}
fn dimension(e: Extent) -> (u32, u32) {
    (e.width, e.height)
}

fn camera() -> Camera {
    Camera {
        view: identity(),
        projection: identity(),
        jitter: Vec2::default(),
    }
}

fn frame(view: u8, source_frame: u64) -> Frame {
    Frame {
        view,
        source_frame,
        sizes: sizes(),
        world: camera(),
        ..Default::default()
    }
}

fn pose() -> Pose {
    Pose {
        world: identity(),
        ..Default::default()
    }
}
fn key(owner: u64, visual: u64, generation: u64) -> Key {
    Key {
        owner,
        visual,
        generation,
    }
}

fn token(t: Token) -> (u64, u64, u64, u8) {
    (t.source_frame, t.serial, t.owner, t.view)
}

pub(crate) fn swap_camera(
    projection: &[f32; 16],
    view: &[f32; 16],
    x: f32,
    y: f32,
    size: [u32; 2],
) -> Option<([f32; 16], [f32; 16])> {
    let column = Matrix {
        values: std::array::from_fn(|i| f64::from(projection[(i % 4) * 4 + i / 4])),
    };
    let jitter = oracle::math::jitter(
        &column,
        f64::from(x),
        f64::from(y),
        extent(size[0], size[1]),
    )?;
    let row: [f32; 16] = std::array::from_fn(|i| jitter.values[(i % 4) * 4 + i / 4] as f32);
    if !row.into_iter().all(f32::is_finite) {
        return None;
    }
    // Original Fmatrix multiplication order for the separate native view oracle.
    let full = std::array::from_fn(|i| {
        let r = (i / 4) * 4;
        let c = i % 4;
        row[c] * view[r]
            + row[4 + c] * view[r + 1]
            + row[8 + c] * view[r + 2]
            + row[12 + c] * view[r + 3]
    });
    Some((row, full))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_temporal_planning_fixture(selection: *const c_char) -> i32 {
    let selected = if selection.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(selection) }.to_bytes())
    };
    let cases: &[(&str, fn())] = &[
        ("independent_dimensions", math::independent_dimensions),
        ("dimension_validation", math::dimension_validation),
        ("matrix_motion", math::matrix_motion),
        (
            "view_timelines_do_not_follow_present",
            history::view_timelines,
        ),
        (
            "reject_frame_mutation_and_bad_inputs",
            history::reject_frame_mutation,
        ),
        (
            "camera_resets_and_first_person_are_separate",
            history::camera_resets,
        ),
        (
            "rigid_instances_freeze_within_source_frame",
            history::rigid_instances,
        ),
        (
            "lifetime_retirement_missing_visibility_and_view_isolation",
            history::retirement,
        ),
        (
            "bone_palettes_are_owned_and_multipass_frozen",
            history::bones,
        ),
        (
            "detail_deformation_uses_item_lifetime_not_batch_order",
            history::details,
        ),
        (
            "reject_incomplete_or_nonfinite_producer_state",
            history::reject_producer,
        ),
        (
            "first_person_object_history_never_uses_world_projection",
            history::first_person,
        ),
        (
            "explicit_resets_invalidate_tokens_without_resurrecting_lifetimes",
            history::explicit_resets,
        ),
        (
            "inverse_and_projective_math_are_finite_and_ordered",
            math::inverse_and_projective,
        ),
        (
            "default_and_degenerate_camera_state_is_not_evidence",
            history::degenerate_camera,
        ),
        (
            "frame_plans_keep_geometry_output_and_pda_canvases_independent",
            history::frame_plans,
        ),
        (
            "matrix_indices_cannot_alias_other_rows",
            math::matrix_indices,
        ),
    ];
    let mut count = 0;
    for &(name, run) in cases {
        if selected.is_none_or(|value| value == name.as_bytes()) {
            if catch_unwind(run).is_err() {
                eprintln!("FAIL: {name}");
                return 1;
            }
            println!("PASS: {name}");
            count += 1;
        }
    }
    if count == 0 {
        eprintln!("FAIL: unknown test selection");
        return 1;
    }
    println!("{count} Anomaly temporal tests passed");
    0
}
