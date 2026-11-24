// SPDX-License-Identifier: MIT
//! The translucent velocity pass's reporting policy: the refusal and commit log budgets and the
//! per-frame CPU profile with its report thresholds, and the merge pass shaders.

#[cfg(windows)]
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::{LazyLock, Mutex, MutexGuard};

// How many refusals and commits are logged per log attachment.
const REFUSAL_BUDGET: u32 = 12;
const COMMIT_BUDGET: u32 = 3;

// A frame's CPU profile is reported when all its callbacks or a single one took this long.
const REPORT_TOTAL_MS: f64 = 8.0;
const REPORT_PEAK_MS: f64 = 4.0;

/// One admitted draw callback's CPU time from entry to return, the part spent admitting its
/// variant, the part spent preparing resources, its native frame and its record bound.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(dead_code)] // constructed by D3D11 replay
pub(crate) struct ProfileSample {
    pub(crate) elapsed_ms: f64,
    pub(crate) shader_ms: f64,
    pub(crate) resource_ms: f64,
    pub(crate) frame: u32,
    pub(crate) records: u32,
}

/// A frame's accumulated profile. `frame` is the last sampled native frame, kept across frame
/// ends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ProfileReport {
    total_ms: f64,
    shader_ms: f64,
    resource_ms: f64,
    peak_ms: f64,
    frame: u32,
    callbacks: u32,
    largest_records: u32,
}

#[derive(Default)]
struct Reports {
    refusals: u32,
    commits: u32,
}

/// Whether a report is within its budget. A spent budget stays spent until the next log
/// attachment.
fn take(count: &mut u32, budget: u32) -> bool {
    if *count >= budget {
        return false;
    }
    *count += 1;
    true
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

#[cfg(windows)]
#[derive(Clone, Copy)]
pub(crate) struct Log {
    pub(crate) write: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub(crate) user: *mut c_void,
}

// The carrier owns the logger and userdata and registers both for synchronized replay.
#[cfg(windows)]
unsafe impl Send for Log {}

#[cfg(windows)]
impl Log {
    fn emit(self, message: &CStr) {
        // The shared foreign-exception guard keeps a throwing carrier logger out of Rust.
        unsafe {
            rsf_game_sys::rsf_game_log_guarded(Some(self.write), self.user, message.as_ptr())
        };
    }
}

#[derive(Default)]
struct Pass {
    #[cfg(windows)]
    log: Option<Log>,
    reports: Reports,
    profile: Profile,
}

static PASS: LazyLock<Mutex<Pass>> = LazyLock::new(Mutex::default);

fn lock() -> MutexGuard<'static, Pass> {
    PASS.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(windows)]
pub(crate) fn set_log(log: Option<Log>) {
    let mut pass = lock();
    pass.log = log;
    pass.reports = Reports::default();
}

#[cfg(windows)]
fn take_log(refusal: bool) -> Option<Log> {
    let mut pass = lock();
    let log = pass.log?;
    let reports = &mut pass.reports;
    let within = if refusal {
        take(&mut reports.refusals, REFUSAL_BUDGET)
    } else {
        take(&mut reports.commits, COMMIT_BUDGET)
    };
    within.then_some(log)
}

#[cfg(windows)]
pub(crate) fn refused(reason: &CStr) {
    if let Some(log) = take_log(true) {
        let message = format!(
            "AC7 translucent velocity refused: {}",
            reason.to_string_lossy()
        );
        if let Ok(message) = CString::new(message) {
            log.emit(&message);
        }
    }
}

#[cfg(windows)]
pub(crate) fn log_commit() {
    if let Some(log) = take_log(false) {
        log.emit(c"AC7 translucent velocity: opacity-clipped generated-geometry motion and depth committed before native post-processing");
    }
}

#[cfg(windows)]
pub(crate) fn profile_sample(sample: &ProfileSample) {
    lock().profile.sample(sample);
}

/// The totals restart on every renderer frame end, even without a log attachment.
#[cfg(windows)]
pub(crate) fn log_frame_end(shader_objects: u32, models: usize) {
    let (report, log) = {
        let mut pass = lock();
        (pass.profile.end_frame(), pass.log)
    };
    let (Some(report), Some(log)) = (report, log) else {
        return;
    };
    let message = format!(
        "AC7 velocity CPU frame={} callbacks={} total_ms={:.2} shader_ms={:.2} resource_ms={:.2} peak_ms={:.2} largest_records={} models={} variants={} shader_objects={shader_objects}",
        report.frame,
        report.callbacks,
        report.total_ms,
        report.shader_ms,
        report.resource_ms,
        report.peak_ms,
        report.largest_records,
        models,
        crate::velocity_pass_cache::variant_count(),
    );
    if let Ok(message) = CString::new(message) {
        log.emit(&message);
    }
}

/// The renderer that owned the profile was replaced or released (velocity_renderer/replay.rs).
pub(crate) fn reset_profile() {
    lock().profile = Profile::default();
}

#[cfg(windows)]
pub(crate) const MERGE_SOURCE: &str = r#"
Texture2D<float2> Native:register(t0);Texture2D<float2> Layer:register(t1);
float2 main(float4 pixel:SV_Position):SV_Target0 {
    int3 address=int3(int2(pixel.xy),0);float2 layer=Layer.Load(address);
    return layer.x>0 ? layer : Native.Load(address);
}"#;

#[cfg(windows)]
pub(crate) const FULLSCREEN_SOURCE: &str = r#"
float4 main(uint id:SV_VertexID):SV_Position {return float4(id==2?3:-1,id==1?3:-1,0,1);}
"#;

#[cfg(test)]
mod tests;
