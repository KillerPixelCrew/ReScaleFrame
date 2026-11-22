// SPDX-License-Identifier: MIT
//! CPU scenarios for the independent C++ temporal model.
mod abi;
mod history;
mod math;

use abi::{
    rsf_anomaly_plan_camera as Camera, rsf_anomaly_plan_camera_pair as Pair,
    rsf_anomaly_plan_capture as Capture, rsf_anomaly_plan_extent as Extent,
    rsf_anomaly_plan_fixture as Oracle, rsf_anomaly_plan_frame as Frame,
    rsf_anomaly_plan_key as Key, rsf_anomaly_plan_matrix as Matrix, rsf_anomaly_plan_pose as Pose,
    rsf_anomaly_plan_resolutions as Sizes, rsf_anomaly_plan_sample as Sample,
    rsf_anomaly_plan_token as Token, rsf_anomaly_plan_vec2 as Vec2, rsf_anomaly_plan_vec4 as Vec4,
    rsf_anomaly_plan_view as Plan,
};
use std::ffi::{CStr, c_char, c_void};
use std::panic::catch_unwind;
use std::ptr::NonNull;

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

struct History<'a> {
    api: &'a Oracle,
    ptr: NonNull<c_void>,
}

impl<'a> History<'a> {
    fn new(api: &'a Oracle) -> Self {
        Self {
            api,
            ptr: NonNull::new(unsafe { api.create.unwrap()() }).expect("temporal history"),
        }
    }

    fn begin(&self, frame: &Frame) -> (u32, Token) {
        let mut token = Token::default();
        let status = unsafe { self.api.begin.unwrap()(self.ptr.as_ptr(), frame, &mut token) };
        assert_ne!(status, u32::MAX, "native begin failed");
        assert_eq!(token.owner != 0, matches!(status, STARTED | REPEATED));
        (status, token)
    }

    fn start(&self, frame: &Frame) -> Token {
        let (status, token) = self.begin(frame);
        assert_eq!(status, STARTED);
        token
    }

    fn cameras(&self, token: Token, domain: u8) -> Option<Pair> {
        let mut out = Pair::default();
        unsafe { self.api.cameras.unwrap()(self.ptr.as_ptr(), &token, domain, &mut out) }
            .then_some(out)
    }

    fn plan(&self, token: Token) -> Option<Plan> {
        let mut out = Plan::default();
        unsafe { self.api.plan.unwrap()(self.ptr.as_ptr(), &token, &mut out) }.then_some(out)
    }

    fn capture(&self, token: Token, key: Key, pose: &Pose) -> OwnedCapture<'a> {
        let capture = unsafe { self.api.capture.unwrap()(self.ptr.as_ptr(), &token, key, pose) };
        assert!(!capture.owner.is_null(), "native capture failed");
        OwnedCapture {
            api: self.api,
            capture,
        }
    }

    fn retire(&self, key: Key) -> bool {
        unsafe { self.api.retire.unwrap()(self.ptr.as_ptr(), key) }
    }

    fn reset_view(&self, view: u8) -> bool {
        unsafe { self.api.reset_view.unwrap()(self.ptr.as_ptr(), view) }
    }

    fn reset_all(&self) {
        unsafe { self.api.reset_all.unwrap()(self.ptr.as_ptr()) };
    }
}

impl Drop for History<'_> {
    fn drop(&mut self) {
        unsafe { self.api.destroy.unwrap()(self.ptr.as_ptr()) };
    }
}

struct OwnedCapture<'a> {
    api: &'a Oracle,
    capture: Capture,
}
impl OwnedCapture<'_> {
    fn sample(&self, ptr: *const c_void) -> Option<Sample> {
        if ptr.is_null() {
            return None;
        }
        let mut sample = Sample::default();
        unsafe { self.api.sample.unwrap()(ptr, &mut sample) };
        Some(sample)
    }
    fn current(&self) -> Option<Sample> {
        self.sample(self.capture.current)
    }
    fn previous(&self) -> Option<Sample> {
        self.sample(self.capture.previous)
    }
    fn status(&self) -> u32 {
        self.capture.status
    }
    fn handles(&self) -> (*const c_void, *const c_void) {
        (self.capture.current, self.capture.previous)
    }
}
impl Drop for OwnedCapture<'_> {
    fn drop(&mut self) {
        unsafe { self.api.drop_capture.unwrap()(self.capture.owner) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_temporal_planning_fixture(
    selection: *const c_char,
    api: &Oracle,
) -> i32 {
    let selected = if selection.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(selection) }.to_bytes())
    };
    let cases: &[(&str, fn(&Oracle))] = &[
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
            if catch_unwind(|| run(api)).is_err() {
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
