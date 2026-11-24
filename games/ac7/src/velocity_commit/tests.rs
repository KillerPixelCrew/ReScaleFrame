// SPDX-License-Identifier: MIT
use super::*;

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
fn report_budgets_allow_twelve_refusals_and_three_commits() {
    let mut reports = Reports::default();
    for _ in 0..REFUSAL_BUDGET {
        assert!(take(&mut reports.refusals, REFUSAL_BUDGET));
    }
    assert!(!take(&mut reports.refusals, REFUSAL_BUDGET));
    // The budgets are independent.
    for _ in 0..COMMIT_BUDGET {
        assert!(take(&mut reports.commits, COMMIT_BUDGET));
    }
    assert!(!take(&mut reports.commits, COMMIT_BUDGET));
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
