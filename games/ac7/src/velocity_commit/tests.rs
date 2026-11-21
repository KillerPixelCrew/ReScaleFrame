// SPDX-License-Identifier: MIT
use super::*;
use std::ptr::{NonNull, null, null_mut};

fn pass() -> rsf_game_render_pass {
    // SAFETY: Every field of the plain C pass is valid as zero.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.session_id = 1;
    pass.history_key = 2;
    pass.view_key = 3;
    pass.family_key = 4;
    pass.native_frame = 0x1_0000_0005;
    pass.depth = NonNull::<u8>::dangling().as_ptr().cast();
    pass.motion = NonNull::<u8>::dangling().as_ptr().cast();
    pass
}

fn committed(width: u32, height: u32) -> ViewDecision {
    ViewDecision {
        slot: 0,
        action: VIEW_COMMIT,
        width,
        height,
    }
}

fn target(format: u32) -> CommitTarget {
    CommitTarget {
        width: 16,
        height: 8,
        samples: 1,
        array_size: 1,
        format,
    }
}

fn sample(elapsed_ms: f64, frame: u32, records: u32) -> ProfileSample {
    ProfileSample {
        elapsed_ms,
        shader_ms: elapsed_ms / 4.0,
        resource_ms: elapsed_ms / 2.0,
        frame,
        records,
    }
}

#[test]
fn commit_identity_carries_the_pass_keys_and_low_frame() {
    let identity = commit_identity(&pass()).unwrap();
    assert_eq!(
        (
            identity.session_id,
            identity.history_key,
            identity.view_key,
            identity.family_key,
            identity.frame,
        ),
        (1, 2, 3, 4, 5)
    );
    assert_eq!(
        (identity.width, identity.height, identity.padding),
        (0, 0, 0)
    );
}

#[test]
fn commit_needs_both_depth_and_motion() {
    let mut no_motion = pass();
    no_motion.motion = null_mut();
    assert!(commit_identity(&no_motion).is_none());
    let mut no_depth = pass();
    no_depth.depth = null_mut();
    assert!(commit_identity(&no_depth).is_none());
}

#[test]
fn target_must_match_the_view_extent_and_format() {
    let view = committed(16, 8);
    assert!(target_matches(&view, &target(R16G16_UNORM)));
    assert!(target_matches(&view, &target(R16G16_TYPELESS)));
    // R16G16_FLOAT and R16G16_UINT are refused.
    assert!(!target_matches(&view, &target(34)));
    assert!(!target_matches(&view, &target(36)));

    let mut wide = target(R16G16_UNORM);
    wide.width = 17;
    assert!(!target_matches(&view, &wide));
    let mut tall = target(R16G16_UNORM);
    tall.height = 9;
    assert!(!target_matches(&view, &tall));
    let mut multisampled = target(R16G16_UNORM);
    multisampled.samples = 4;
    assert!(!target_matches(&view, &multisampled));
    let mut arrayed = target(R16G16_UNORM);
    arrayed.array_size = 2;
    assert!(!target_matches(&view, &arrayed));

    assert!(!target_matches(&UNAVAILABLE_VIEW, &target(R16G16_UNORM)));
}

#[test]
fn report_budgets_allow_twelve_refusals_and_three_commits() {
    let mut reports = Reports::default();
    for _ in 0..REFUSAL_BUDGET {
        assert!(reports.take(REPORT_REFUSAL));
    }
    assert!(!reports.take(REPORT_REFUSAL));
    // The budgets are independent.
    for _ in 0..COMMIT_BUDGET {
        assert!(reports.take(REPORT_COMMIT));
    }
    assert!(!reports.take(REPORT_COMMIT));
    assert!(!reports.take(2));
    assert_eq!((reports.refusals, reports.commits), (12, 3));
}

#[test]
fn profile_reports_over_the_total_or_peak_threshold() {
    let mut profile = Profile::default();
    profile.sample(&sample(3.0, 7, 100));
    profile.sample(&sample(3.0, 7, 400));
    assert_eq!(profile.end_frame(), None);

    profile.sample(&sample(4.0, 8, 10));
    let peak = profile.end_frame().unwrap();
    assert_eq!(
        (
            peak.frame,
            peak.callbacks,
            peak.largest_records,
            peak.peak_ms
        ),
        (8, 1, 10, 4.0)
    );

    for _ in 0..3 {
        profile.sample(&sample(2.75, 9, 50));
    }
    let total = profile.end_frame().unwrap();
    assert_eq!(
        (
            total.callbacks,
            total.total_ms,
            total.shader_ms,
            total.resource_ms
        ),
        (3, 8.25, 2.0625, 4.125)
    );
    assert_eq!(total.peak_ms, 2.75);
}

#[test]
fn profile_totals_restart_at_frame_end_but_keep_the_frame() {
    let mut profile = Profile::default();
    profile.sample(&sample(9.0, 11, 64));
    assert!(profile.end_frame().is_some());
    assert_eq!(
        profile.report,
        ProfileReport {
            frame: 11,
            ..ProfileReport::default()
        }
    );
    assert_eq!(profile.end_frame(), None);
}

#[test]
fn exported_calls_reject_missing_and_short_inputs() {
    let mut decision = committed(1, 1);
    // SAFETY: Null pointers are the tested refusal.
    assert_eq!(
        unsafe { rsf_ac7_velocity_commit_view(null(), &mut decision) },
        0
    );
    assert_eq!(decision, UNAVAILABLE_VIEW);

    let mut short = pass();
    short.struct_size -= 1;
    decision = committed(1, 1);
    // SAFETY: A full pass whose size field is short is the tested refusal.
    assert_eq!(
        unsafe { rsf_ac7_velocity_commit_view(&short, &mut decision) },
        0
    );
    assert_eq!(decision, UNAVAILABLE_VIEW);

    // SAFETY: Null pointers are the tested refusal.
    unsafe {
        assert_eq!(rsf_ac7_velocity_commit_view(&pass(), null_mut()), 0);
        assert_eq!(
            rsf_ac7_velocity_commit_target(null(), &target(R16G16_UNORM)),
            0
        );
        assert_eq!(rsf_ac7_velocity_commit_target(&committed(16, 8), null()), 0);
        assert_eq!(
            rsf_ac7_velocity_commit_target(&committed(16, 8), &target(R16G16_UNORM)),
            1
        );
        rsf_ac7_velocity_profile_sample(null());
    }
}
