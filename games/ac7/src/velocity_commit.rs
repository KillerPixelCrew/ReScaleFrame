// SPDX-License-Identifier: MIT
//! The translucent velocity pass's commit and reporting policy: the commit pass's admission and
//! its view identity, the native motion target's extent, sample, array and format check, the
//! refusal and commit log budgets, and the per-frame CPU profile with its report thresholds.
//! velocity_pass.cpp keeps the renderer and context checks, the view's native depth identity, the
//! target's description and device check, the state scope, the merge draw, the copies, the clock,
//! the shader object count and every log line's text.

use crate::velocity_history::{self, UNAVAILABLE_VIEW, VIEW_COMMIT, ViewDecision, ViewDraw};
use rsf_game_sys::rsf_game_render_pass;
use std::mem::offset_of;
use std::sync::{LazyLock, Mutex, MutexGuard};

// DXGI_FORMAT_R16G16_TYPELESS and DXGI_FORMAT_R16G16_UNORM, the engine's velocity target.
const R16G16_TYPELESS: u32 = 33;
const R16G16_UNORM: u32 = 35;

// Report kinds and how many of each are logged per log attachment.
const REPORT_REFUSAL: u32 = 0;
const REPORT_COMMIT: u32 = 1;
const REFUSAL_BUDGET: u32 = 12;
const COMMIT_BUDGET: u32 = 3;

// A frame's CPU profile is reported when all its callbacks or a single one took this long.
const REPORT_TOTAL_MS: f64 = 8.0;
const REPORT_PEAK_MS: f64 = 4.0;

/// The native motion target's description: width, height, sample count, array size and
/// DXGI format.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)] // constructed by velocity_pass.cpp
struct CommitTarget {
    width: u32,
    height: u32,
    samples: u32,
    array_size: u32,
    format: u32,
}

/// One admitted draw callback's CPU time from entry to return, the part spent admitting its
/// variant, the part spent preparing resources, its native frame and its record bound.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(dead_code)] // constructed by velocity_pass.cpp
struct ProfileSample {
    elapsed_ms: f64,
    shader_ms: f64,
    resource_ms: f64,
    frame: u32,
    records: u32,
}

/// A frame's accumulated profile. `frame` is the last sampled native frame, kept across frame
/// ends.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ProfileReport {
    total_ms: f64,
    shader_ms: f64,
    resource_ms: f64,
    peak_ms: f64,
    frame: u32,
    callbacks: u32,
    largest_records: u32,
    padding: u32,
}

const _: () = {
    assert!(size_of::<CommitTarget>() == 20);
    assert!(size_of::<ProfileSample>() == 32 && offset_of!(ProfileSample, frame) == 24);
    assert!(size_of::<ProfileReport>() == 48 && offset_of!(ProfileReport, frame) == 32);
};

/// The view identity of a complete pass that leases both depth and motion.
fn commit_identity(pass: &rsf_game_render_pass) -> Option<ViewDraw> {
    if pass.motion.is_null() || pass.depth.is_null() {
        return None;
    }
    Some(ViewDraw {
        session_id: pass.session_id,
        history_key: pass.history_key,
        view_key: pass.view_key,
        family_key: pass.family_key,
        // The history keeps the low 32 bits, as the draw does.
        frame: pass.native_frame as u32,
        width: 0,
        height: 0,
        padding: 0,
    })
}

/// A single-sample, unarrayed R16G16 UNORM or typeless target at the committed view's extent.
fn target_matches(view: &ViewDecision, target: &CommitTarget) -> bool {
    view.action == VIEW_COMMIT
        && target.width == view.width
        && target.height == view.height
        && target.samples == 1
        && target.array_size == 1
        && matches!(target.format, R16G16_UNORM | R16G16_TYPELESS)
}

#[derive(Default)]
struct Reports {
    refusals: u32,
    commits: u32,
}

impl Reports {
    /// Whether a report of this kind is within its budget. A spent budget stays spent until the
    /// next log attachment.
    fn take(&mut self, kind: u32) -> bool {
        let (count, budget) = match kind {
            REPORT_REFUSAL => (&mut self.refusals, REFUSAL_BUDGET),
            REPORT_COMMIT => (&mut self.commits, COMMIT_BUDGET),
            _ => return false,
        };
        if *count >= budget {
            return false;
        }
        *count += 1;
        true
    }
}

#[derive(Default)]
struct Profile {
    report: ProfileReport,
}

impl Profile {
    fn sample(&mut self, sample: &ProfileSample) {
        let report = &mut self.report;
        report.frame = sample.frame;
        report.total_ms += sample.elapsed_ms;
        report.shader_ms += sample.shader_ms;
        report.resource_ms += sample.resource_ms;
        report.peak_ms = report.peak_ms.max(sample.elapsed_ms);
        report.callbacks = report.callbacks.wrapping_add(1);
        report.largest_records = report.largest_records.max(sample.records);
    }

    /// The frame's profile when it crossed a threshold. The totals restart either way.
    fn end_frame(&mut self) -> Option<ProfileReport> {
        let report = self.report;
        self.report = ProfileReport {
            frame: report.frame,
            ..ProfileReport::default()
        };
        (report.total_ms >= REPORT_TOTAL_MS || report.peak_ms >= REPORT_PEAK_MS).then_some(report)
    }
}

#[derive(Default)]
struct Pass {
    reports: Reports,
    profile: Profile,
}

static PASS: LazyLock<Mutex<Pass>> = LazyLock::new(Mutex::default);

fn lock() -> MutexGuard<'static, Pass> {
    PASS.lock().unwrap_or_else(|error| error.into_inner())
}

/// Nonzero with the decision's slot and extent when the pass's view has translucent motion
/// written for its frame, view and family. The decision is unavailable otherwise.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_commit_view(
    pass: *const rsf_game_render_pass,
    decision: *mut ViewDecision,
) -> i32 {
    // SAFETY: The commit lends its decision for this call.
    let Some(decision) = (unsafe { decision.as_mut() }) else {
        return 0;
    };
    *decision = UNAVAILABLE_VIEW;
    // SAFETY: The commit lends its pass for this call; only its leading size is read before the
    // size check admits the rest.
    if pass.is_null() || unsafe { (*pass).struct_size } < size_of::<rsf_game_render_pass>() as u32 {
        return 0;
    }
    // SAFETY: Non-null and at least the full pass, checked above.
    let Some(identity) = commit_identity(unsafe { &*pass }) else {
        return 0;
    };
    *decision = velocity_history::commit(&identity);
    i32::from(decision.action == VIEW_COMMIT)
}

/// Nonzero when the native motion target can receive the committed view's merge.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_commit_target(
    view: *const ViewDecision,
    target: *const CommitTarget,
) -> i32 {
    // SAFETY: The commit lends both for this call.
    let (Some(view), Some(target)) = (unsafe { view.as_ref() }, unsafe { target.as_ref() }) else {
        return 0;
    };
    i32::from(target_matches(view, target))
}

/// Nonzero when a report of this kind may be logged; it counts against the budget.
#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_report(kind: u32) -> i32 {
    i32::from(lock().reports.take(kind))
}

/// A new log attachment restores every budget.
#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_reports_reset() {
    lock().reports = Reports::default();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_profile_sample(sample: *const ProfileSample) {
    // SAFETY: The finishing draw callback lends its sample for this call.
    if let Some(sample) = unsafe { sample.as_ref() } {
        lock().profile.sample(sample);
    }
}

/// Nonzero with the report written when the frame crossed a threshold. The totals restart
/// either way, also without an output.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_profile_frame_end(output: *mut ProfileReport) -> i32 {
    let Some(report) = lock().profile.end_frame() else {
        return 0;
    };
    // SAFETY: The frame end lends its report for this call.
    match unsafe { output.as_mut() } {
        Some(output) => {
            *output = report;
            1
        }
        None => 0,
    }
}

/// The renderer that owned the profile was replaced or released.
#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_profile_reset() {
    lock().profile = Profile::default();
}

#[cfg(test)]
mod tests;
