// SPDX-License-Identifier: MIT
//! Bounded diagnostics over copied CPU frame and provider facts.
use super::{
    AntiLag, Facts, Frame, LEDGER, OK, Pass, Platform, Reservation, Source, antilag, lock,
};
use crate::cpu_abi::rsf_native_cpu_counters;
use crate::native_cpu::counters;
use crate::native_fg::{now_qpc, requested, sink};
use crate::render_abi::RSF_FG_OFF;
use rsf_game_sys::{
    RSF_GAME_CPU_INPUT_SAMPLE, RSF_GAME_CPU_SIMULATION_BEGIN, RSF_GAME_CPU_SIMULATION_END,
};
use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::mem::zeroed;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

pub(super) struct Diagnostics {
    ownership: [OwnershipTotals; 9],
    reported: rsf_native_cpu_counters,
    reported_anti: (u64, u64),
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            ownership: [OwnershipTotals::default(); 9],
            reported: unsafe { zeroed() },
            reported_anti: (0, 0),
        }
    }
}

thread_local! {
    static TIMING: RefCell<Timing> = RefCell::new(Timing::default());
}

#[derive(Clone, Copy, Default)]
struct OwnershipTotals {
    epoch: u64,
    frames: u64,
    expected: u64,
    scenes: u64,
    ends: u64,
    sr: u64,
    presents: u64,
    windows: u64,
    unpresented: u64,
    multi: u64,
    mismatched: u64,
    native_gaps: u64,
    rhi_begin: u64,
    rhi_end: u64,
    previous_native: u32,
}

pub(super) fn audit(p: &Platform, source: Source) {
    let now = now_qpc();
    let frequency = call!(p, qpc_frequency);
    let (frame, total) = {
        let mut ledger = lock(&LEDGER);
        let Some(frame) = ledger.exact(source) else {
            return;
        };
        if frame.audited || !frame.ended || frame.rhi_ends == 0 {
            return;
        }
        frame.audited = true;
        let frame = *frame;
        let total = &mut ledger.diagnostics.ownership[frame.screen.min(8) as usize];
        if total.epoch == 0 {
            total.epoch = now;
        }
        total.frames += 1;
        total.expected += u64::from(frame.expected_count);
        total.scenes += u64::from(frame.scene_begins);
        total.ends += u64::from(frame.scene_ends);
        total.sr += u64::from(frame.sr_calls);
        total.presents += u64::from(frame.presents);
        total.windows += u64::from(frame.windows);
        total.rhi_begin += u64::from(frame.rhi_begins);
        total.rhi_end += u64::from(frame.rhi_ends);
        total.unpresented += u64::from(frame.presents == 0);
        total.mismatched += u64::from(frame.mismatched_window);
        total.multi +=
            u64::from(frame.scene_begins > 1 || frame.sr_calls > 1 || frame.presents > 1);
        if frame.native_frame != 0 {
            total.native_gaps += u64::from(
                total.previous_native != 0
                    && frame.native_frame != total.previous_native.wrapping_add(1),
            );
            total.previous_native = frame.native_frame;
        }
        if now.saturating_sub(total.epoch) < frequency {
            return;
        }
        let snapshot = *total;
        *total = OwnershipTotals {
            previous_native: total.previous_native,
            epoch: now,
            ..OwnershipTotals::default()
        };
        (frame, snapshot)
    };
    let log = sink(true);
    if log.available() {
        log.say(&format!(
            "FG ownership screen={} source={} native={} frames={} expected={} scene_begin={} scene_end={} sr={} presents={} windows={} rhi_begin={} rhi_end={} no_present={} multi={} window_mismatch={} native_gaps={} submission={}..{} view={:x}..{:x}",
            frame.screen, source.id, frame.native_frame, total.frames, total.expected, total.scenes,
            total.ends, total.sr, total.presents, total.windows, total.rhi_begin, total.rhi_end,
            total.unpresented, total.multi, total.mismatched, total.native_gaps, frame.first_submission,
            frame.last_submission, frame.first_view, frame.last_view,
        ));
    }
}

#[derive(Default)]
struct Timing {
    start: u64,
    frames: u64,
    sum: [f64; 8],
    peak: [f64; 8],
}

pub(crate) fn report_timing(
    p: &Platform,
    cpu: &Facts,
    owner: &Frame,
    screen: u32,
    wait: u64,
    evaluate: u64,
) {
    if cpu.qpc_frequency == 0
        || owner.ready_qpc == 0
        || owner.submit_begin == 0
        || owner.submit_end < owner.submit_begin
    {
        return;
    }
    let now = now_qpc();
    let side = |stage: u32| cpu.timestamps_qpc[stage as usize];
    let input = side(RSF_GAME_CPU_INPUT_SAMPLE);
    let sim = side(RSF_GAME_CPU_SIMULATION_BEGIN);
    let spans = [
        owner.join_ticks,
        if owner.prospective && !owner.paced {
            0
        } else {
            owner.reflex_ticks
        },
        input.saturating_sub(owner.ready_qpc),
        sim.saturating_sub(input),
        side(RSF_GAME_CPU_SIMULATION_END).saturating_sub(sim),
        owner.submit_end - owner.submit_begin,
        evaluate,
        wait,
    ];
    let scale = 1000.0 / cpu.qpc_frequency as f64;
    let snapshot = TIMING.with_borrow_mut(|timing| {
        if timing.start == 0 {
            timing.start = now;
        }
        for (i, span) in spans.into_iter().enumerate() {
            let ms = span as f64 * scale;
            timing.sum[i] += ms;
            timing.peak[i] = timing.peak[i].max(ms);
        }
        timing.frames += 1;
        (now.saturating_sub(timing.start) as f64 * scale >= 1000.0).then(|| {
            std::mem::replace(
                timing,
                Timing {
                    start: now,
                    ..Timing::default()
                },
            )
        })
    });
    let log = sink(false);
    if let Some(timing) = snapshot
        && log.available()
    {
        let avg = |i: usize| timing.sum[i] / timing.frames as f64;
        let peak = timing.peak;
        let idle = call!(p, input_idle_ms);
        log.say(&format!(
            "FG source timing source={} frames={} screen={screen} windows_input_idle_ms={idle} join_ms={:.2}/{:.2} pacing_operation_ms={:.2}/{:.2} engine_preinput_ms={:.2}/{:.2} input_to_sim_ms={:.2}/{:.2} simulation_ms={:.2}/{:.2} rhi_ms={:.2}/{:.2} sr_ms={:.2}/{:.2} sr_wait_ms={:.2}/{:.2}",
            owner.id, timing.frames, avg(0), peak[0], avg(1), peak[1], avg(2), peak[2], avg(3), peak[3],
            avg(4), peak[4], avg(5), peak[5], avg(6), peak[6], avg(7), peak[7],
        ));
    }
}

pub(super) fn report_latency_window(p: &Platform) {
    let totals = counters();
    let (reported, anti, previous_anti) = {
        let mut ledger = lock(&LEDGER);
        let mut reported = ledger.diagnostics.reported;
        if totals.frames < reported.frames {
            reported = unsafe { zeroed() };
        } else if totals.frames - reported.frames < 120 {
            return;
        }
        let anti = antilag(p).map(AntiLag::state).unwrap_or_default();
        let previous_anti = ledger.diagnostics.reported_anti;
        ledger.diagnostics.reported = totals;
        ledger.diagnostics.reported_anti = (anti.updates, anti.refused);
        (reported, anti, previous_anti)
    };
    let log = sink(false);
    if log.available() {
        let delta = u64::wrapping_sub;
        log.say(&format!(
            "Latency window: inputs_keyboard={} mouse={} controller={} pcl_ping={} substitute={} duplicate={} marker_order_violations={} pcl_expired={} Anti-Lag2_updates={} Anti-Lag2_refused={}",
            delta(totals.input_events[0], reported.input_events[0]),
            delta(totals.input_events[1], reported.input_events[1]),
            delta(totals.input_events[2], reported.input_events[2]), delta(totals.pings, reported.pings),
            delta(totals.substitutes, reported.substitutes), delta(totals.duplicates, reported.duplicates),
            delta(totals.order_violations, reported.order_violations),
            delta(totals.expired_messages, reported.expired_messages), delta(anti.updates, previous_anti.0),
            delta(anti.refused, previous_anti.1),
        ));
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Failure {
    SrPrerequisite,
    SrToken,
    SrInitialize,
    SrSharedEvaluate,
    PaceDependency,
    PaceSelection,
    PaceCall,
    RenderCpu,
    RenderProspective,
    RenderInput,
    RenderProvider,
    RenderOwner,
}

impl Failure {
    fn name(self) -> &'static str {
        match self {
            Self::SrPrerequisite => "sr-prerequisite",
            Self::SrToken => "sr-token",
            Self::SrInitialize => "sr-initialize",
            Self::SrSharedEvaluate => "sr-shared-evaluate",
            Self::PaceDependency => "pace-dependency",
            Self::PaceSelection => "pace-selection",
            Self::PaceCall => "pace-call",
            Self::RenderCpu => "render-cpu",
            Self::RenderProspective => "render-prospective",
            Self::RenderInput => "render-input",
            Self::RenderProvider => "render-provider",
            Self::RenderOwner => "render-owner",
        }
    }
}

pub(crate) fn sr_failure(stage: Failure, source: u64, session: u64, result: i32, detail: u32) {
    failure(
        stage,
        Source {
            id: source,
            session,
        },
        result,
        detail,
        0,
        None,
        None,
    );
}

pub(super) fn failure(
    stage: Failure,
    source: Source,
    result: i32,
    detail: u32,
    generation: u64,
    cpu: Option<&Facts>,
    reservation: Option<&Reservation>,
) {
    static REPORTED: AtomicU32 = AtomicU32::new(0);
    let bit = 1 << (stage as u32);
    if REPORTED.load(Ordering::Relaxed) & bit != 0 {
        return;
    }
    let log = sink(false);
    if !log.available() || REPORTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    log.say(&format!(
        "Native failure stage={} source={} session={} session_known={} result={result} detail=0x{detail:x} generation_known={} generation={generation} cpu_known={} cpu_mask={} marker_mask={} cpu_flags={} cpu_failed={} reservation_known={} reservation_state={} predecessor_present={}",
        stage.name(), source.id, source.session, u32::from(source.session != 0), u32::from(generation != 0),
        u32::from(cpu.is_some()), cpu.map_or(0, |c| c.stage_mask), cpu.map_or(0, |c| c.marker_mask),
        cpu.map_or(0, |c| c.flags), cpu.map_or(0, |c| c.failed), u32::from(reservation.is_some()),
        reservation.map_or(0, |r| r.state), reservation.map_or(0, |r| r.predecessor_present),
    ));
}

pub(crate) fn trace(stage: &str, id: u64, result: i32) {
    use std::time::{Duration, Instant};
    static TRACE_BUDGET: Mutex<(u32, Option<Instant>)> = Mutex::new((0, None));
    let debug = requested().debug != 0;
    let due = {
        let mut budget = lock(&TRACE_BUDGET);
        let mut error_due = false;
        if result != OK {
            let now = Instant::now();
            error_due = budget
                .1
                .is_none_or(|previous| now.duration_since(previous) >= Duration::from_secs(1));
            if error_due {
                budget.1 = Some(now);
            }
        }
        let due = debug && (budget.0 < 256 || error_due);
        budget.0 = budget.0.saturating_add(u32::from(debug));
        due
    };
    if due {
        sink(false).say(&format!("FG/Reflex {stage} source={id} result={result}"));
    }
}

#[derive(Default)]
struct StageBudget {
    generation: u64,
    lines: u32,
    stage_lines: u32,
    prepare_lines: u32,
    prepare_calls: u32,
}

pub(crate) fn stage(
    p: &Platform,
    generation: u64,
    stage: &str,
    source: u64,
    native: u64,
    result: i32,
    prepare: Option<(&Frame, &Pass)>,
) {
    if requested().mode == RSF_FG_OFF {
        return;
    }
    let log = sink(false);
    if !log.available() {
        return;
    }
    static BUDGET: LazyLock<Mutex<StageBudget>> =
        LazyLock::new(|| Mutex::new(StageBudget::default()));
    {
        let mut budget = lock(&BUDGET);
        if generation != budget.generation {
            *budget = StageBudget {
                generation,
                ..StageBudget::default()
            };
        }
        if prepare.is_some() {
            // Keep rows for matching late-switch producer samples as well as startup.
            let first = budget.prepare_calls < 8;
            budget.prepare_calls = budget.prepare_calls.saturating_add(1);
            if (!first && (source == 0 || !source.is_multiple_of(128)))
                || budget.prepare_lines >= 64
            {
                return;
            }
            budget.prepare_lines += 1;
        } else {
            if budget.stage_lines >= 32 {
                return;
            }
            budget.stage_lines += 1;
        }
        if budget.lines >= 96 {
            return;
        }
        budget.lines += 1;
    }
    let thread = call!(p, thread_id);
    let mut row = format!(
        "FG stage={stage} generation={generation} source={source} native_frame={native} result={result} thread={thread}"
    );
    if let Some((owner, window)) = prepare {
        // Read-only snapshots taken only after admission to the diagnostics budget.
        let source = Source::from_pass(window);
        let cpu = source.facts();
        let reservation = source.reservation();
        use std::fmt::Write;
        let _ = write!(
            row,
            " owner_id={} owner_session={} window_session={} submitted={} render_complete={} paced={} prospective={} owner_failed={} owner_generation={} provider_generation={generation} cpu_read={} cpu_failed={} cpu_mask={} cpu_flags={} reservation_read={} reservation_state={} reservation_abort={} predecessor_present={} poll_begin={} owner_ready={}",
            owner.id,
            owner.session,
            source.session,
            u32::from(owner.submitted),
            u32::from(owner.render_complete),
            u32::from(owner.paced),
            u32::from(owner.prospective),
            u32::from(owner.failed),
            owner.provider_generation,
            u32::from(cpu.is_some()),
            cpu.map_or(0, |f| f.failed),
            cpu.map_or(0, |f| f.stage_mask),
            cpu.map_or(0, |f| f.flags),
            u32::from(reservation.is_some()),
            reservation.map_or(0, |r| r.state),
            reservation.map_or(0, |r| r.abort_reason),
            reservation.map_or(0, |r| r.predecessor_present),
            reservation.map_or(0, |r| r.poll_begin_qpc),
            owner.ready_qpc,
        );
    }
    log.say(&row);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_stage(
    name: *const c_char,
    source: u64,
    native: u64,
    result: i32,
    p: &Platform,
) {
    if !name.is_null() {
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        stage(p, call!(p, generation), &name, source, native, result, None);
    }
}
