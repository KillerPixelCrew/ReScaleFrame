// SPDX-License-Identifier: MIT
//! Direct3D pacing and submission ownership. The dispatcher in native_cpu
//! owns producer facts and reservations. This owner tracks attempts and the
//! provider generation that consumed those facts.
use crate::cpu_abi::*;
use crate::fg12_abi::rsf_native_latency_frame as LatencyFrame;
use crate::latency_antilag::Context as AntiLag;
use crate::latency_lease::{
    rsf_core_latency_pacing_owner, rsf_native_latency_claim, rsf_native_latency_release_claim,
};
use crate::native_cpu::{
    counters, frame_facts, mark_frame, marker_prerequisites, record_pacing, rendered_source,
    reservation, rsf_native_cpu_is_ping_message, rsf_native_cpu_ping_message,
    rsf_native_cpu_release_session,
};
use crate::native_fg::{now_qpc, requested};
use crate::native_fg_cpu_abi::*;
use crate::overlay_abi::{
    RSF_REFLEX_ON, rsf_native_input_routes as Routes, rsf_native_latency_status as LatencyStatus,
};
use crate::render_abi::{
    RSF_FG_FIXED, RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_PACING_XELL, RSF_REFLEX_OFF,
    rsf_fg_status as Status,
};
use rsf_game_sys::*;
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::zeroed;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};

type Platform = rsf_nfg_cpu_platform;
type Event = rsf_game_cpu_event;
type Pass = rsf_game_render_pass;
type Owner = rsf_nfg_receipt_candidate;
type Reservation = rsf_native_cpu_reservation;
type Facts = rsf_native_cpu_frame;
type Window = rsf_native_latency_window;
type Handle = *mut c_void;

const CAPACITY: usize = RSF_NFG_CPU_CAPACITY as usize;
const D3D_CLAIM: u32 = 1;
const OK: i32 = 0;
const NOT_READY: i32 = RSF_NFG_CPU_NOT_READY;
const INVALID: i32 = RSF_NFG_CPU_INVALID;
const ACTUAL_INPUT: u32 = (1 << RSF_GAME_CPU_FRAME_BEGIN)
    | (1 << RSF_GAME_CPU_INPUT_SAMPLE)
    | (1 << RSF_GAME_CPU_SIMULATION_BEGIN);
const SIM_INTERVAL: u32 = ACTUAL_INPUT | (1 << RSF_GAME_CPU_SIMULATION_END);

macro_rules! call {
    ($p:expr, $f:ident $(, $a:expr)* $(,)?) => {{
        #[allow(unused_unsafe)]
        let value = unsafe { ($p.$f.expect(concat!("native FG CPU platform entry ", stringify!($f))))($($a),*) };
        value
    }};
}

mod diagnostics;
mod hitch;
use diagnostics::{Diagnostics, audit, failure, report_latency_window};
pub(crate) use diagnostics::{Failure, report_timing, sr_failure, stage, trace};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Source {
    session: u64,
    id: u64,
}

impl Source {
    fn from_event(event: &Event) -> Self {
        Self {
            session: event.session_id,
            id: event.source_frame_id,
        }
    }

    fn from_pass(pass: &Pass) -> Self {
        Self {
            session: pass.session_id,
            id: pass.source_frame_id,
        }
    }

    fn facts(self) -> Option<Facts> {
        frame_facts(self.session, self.id)
    }

    fn reservation(self) -> Option<Reservation> {
        reservation(self.session, self.id)
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Frame {
    pub(crate) id: u64,
    pub(crate) session: u64,
    pub(crate) provider_generation: u64,
    pub(crate) ready_qpc: u64,
    pub(crate) source_qpc: u64,
    pub(crate) source_frequency: u64,
    pub(crate) source_delta_ms: f32,
    pub(crate) first_submission: u64,
    pub(crate) first_view: u64,
    pub(crate) submitted: bool,
    pub(crate) render_complete: bool,
    pub(crate) paced: bool,
    pub(crate) prospective: bool,
    pub(crate) failed: bool,
    pub(crate) rhi_begins: u32,
    pub(crate) scene_begins: u32,
    pub(crate) scene_ends: u32,
    ended: bool,
    pacing_started: bool,
    pacing_finished: bool,
    render_started: bool,
    render_starting: bool,
    submit_ended: bool,
    submit_ending: bool,
    present_started: bool,
    present_starting: bool,
    present_ended: bool,
    present_ending: bool,
    audited: bool,
    expected_count: u32,
    sr_calls: u32,
    presents: u32,
    windows: u32,
    rhi_ends: u32,
    screen: u32,
    native_frame: u32,
    mismatched_window: u32,
    last_submission: u64,
    last_view: u64,
    join_ticks: u64,
    reflex_ticks: u64,
    submit_begin: u64,
    submit_end: u64,
    previous: u64,
}

impl Frame {
    fn source(self) -> Source {
        Source {
            id: self.id,
            session: self.session,
        }
    }

    fn pacing_in_flight(self) -> bool {
        self.pacing_started && !self.pacing_finished
    }
}

#[derive(Clone, Copy, Default)]
struct Binding {
    key: u64,
    session: u64,
    thread: u32,
    epoch: u64,
}

struct Ledger {
    frames: [Frame; CAPACITY],
    binding: Binding,
    routes: Routes,
    last_begun: Source,
    last_rendered: (u64, u64, u64),
    diagnostics: Diagnostics,
}

impl Ledger {
    fn new() -> Self {
        Self {
            frames: [Frame::default(); CAPACITY],
            binding: Binding::default(),
            routes: empty_routes(),
            last_begun: Source::default(),
            last_rendered: (0, 0, 0),
            diagnostics: Diagnostics::default(),
        }
    }

    fn slot(&mut self, id: u64) -> &mut Frame {
        &mut self.frames[id as usize % CAPACITY]
    }

    fn frame(&mut self, id: u64) -> Option<&mut Frame> {
        let frame = self.slot(id);
        (id != 0 && frame.id == id).then_some(frame)
    }

    fn exact(&mut self, source: Source) -> Option<&mut Frame> {
        self.frame(source.id)
            .filter(|frame| frame.session == source.session)
    }

    fn claim_pace(&mut self, source: Source) -> Option<Frame> {
        let frame = self.exact(source)?;
        if frame.pacing_started || frame.failed {
            return None;
        }
        frame.pacing_started = true;
        Some(*frame)
    }

    fn rendered_begin(&mut self, pass: &Pass, generation: u64, qpc: u64, frequency: u64) {
        let last = self.last_rendered;
        let source = Source::from_pass(pass);
        let Some(frame) = self.exact(source) else {
            return;
        };
        frame.rhi_begins += 1;
        if frame.rhi_begins != 1 {
            frame.failed = true;
            return;
        }
        frame.source_qpc = qpc;
        frame.source_frequency = frequency;
        if frequency != 0
            && last.0 == pass.session_id
            && last.1.checked_add(1) == Some(pass.native_frame)
            && qpc > last.2
        {
            frame.source_delta_ms = ((qpc - last.2) as f64 * 1000.0 / frequency as f64) as f32;
        }
        frame.provider_generation = generation;
        self.last_rendered = (pass.session_id, pass.native_frame, qpc);
    }
}

static LEDGER: LazyLock<Mutex<Ledger>> = LazyLock::new(|| Mutex::new(Ledger::new()));
static THREADED: AtomicBool = AtomicBool::new(false);
static GRAPHICS_API: AtomicU32 = AtomicU32::new(11);
static DERIVED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static JOIN_SKIP: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static RHI_SOURCE: Cell<Source> = const { Cell::new(Source { session: 0, id: 0 }) };
    static SHOWN: Cell<(u32, u64)> = const { Cell::new((u32::MAX, 0)) };
}

pub(crate) fn frame_read(source: u64) -> Frame {
    lock(&LEDGER).frame(source).copied().unwrap_or_default()
}

pub(crate) fn count_sr(source: u64) {
    if let Some(frame) = lock(&LEDGER).frame(source) {
        frame.sr_calls += 1;
    }
}

pub(crate) fn count_window(source: u64) {
    if let Some(frame) = lock(&LEDGER).frame(source) {
        frame.windows += 1;
    }
}

pub(crate) fn fail(source: u64) {
    if let Some(frame) = lock(&LEDGER).frame(source) {
        frame.failed = true;
    }
}

pub(crate) fn present_observed(presenting: u64, session: u64) {
    let rhi = RHI_SOURCE.get();
    let source = if presenting != 0 {
        Source {
            id: presenting,
            session,
        }
    } else {
        rhi
    };
    if let Some(frame) = lock(&LEDGER).exact(source) {
        frame.presents += 1;
        frame.mismatched_window += u32::from(presenting == 0 || source != rhi);
    }
}

pub(crate) fn complete(p: &Platform, source: u64) {
    COMPLETED.fetch_max(source, Ordering::AcqRel);
    call!(p, signal_completion);
}

pub(crate) fn progress(p: &Platform, source: u64) -> (bool, bool) {
    let host = call!(p, host);
    if host.is_null() {
        (
            source != 0 && COMPLETED.load(Ordering::Acquire) < source,
            false,
        )
    } else {
        let live = !call!(p, host_token, host, source).is_null();
        (live, live && call!(p, host_presented, host, source) != 0)
    }
}

fn frame_acquire(p: &Platform, source: Source) -> i32 {
    if source.id == 0 {
        return INVALID;
    }
    let result = call!(p, acquire, source.id);
    let host = call!(p, host);
    if result != NOT_READY || host.is_null() {
        return result;
    }
    let oldest = lock(&LEDGER)
        .frames
        .iter()
        .copied()
        .filter(|f| {
            f.id != 0
                && f.id < source.id
                && f.session == source.session
                && !f.failed
                && !f.render_started
                && !f.render_starting
                && !f.submitted
                && !f.pacing_in_flight()
                && f.source().facts().is_some_and(|facts| facts.ended != 0)
        })
        .min_by_key(|f| f.id);
    if let Some(oldest) = oldest
        && call!(p, host_abort, host, oldest.id) == OK
    {
        if let Some(frame) = lock(&LEDGER).exact(oldest.source()) {
            frame.failed = true;
        }
        DERIVED.fetch_add(1, Ordering::Relaxed);
        trace(
            "unsubmitted latency source retired at token capacity",
            oldest.id,
            NOT_READY,
        );
        return call!(p, acquire, source.id);
    }
    result
}

fn frame_marker(p: &Platform, frame: Frame, marker: u32) -> (i32, bool) {
    if rendered_source() {
        return (OK, true);
    }
    let generation = frame.provider_generation;
    if frame.failed || frame.pacing_in_flight() || generation != call!(p, generation) {
        return (NOT_READY, false);
    }
    match mark_frame(Some(frame.session), frame.id, marker) {
        1 => {
            let result = call!(p, marker, generation, frame.id, marker, 0);
            (result, result == OK)
        }
        0 => (OK, false),
        _ => (INVALID, false),
    }
}

// Drain only sent messages while joining an ordered Present. Posted input
// remains queued for the engine's actual input callback after pacing.
fn join_previous_present(p: &Platform, previous: u64) -> u64 {
    if previous == 0 || COMPLETED.load(Ordering::Acquire) >= previous {
        return 0;
    }
    let mut skip = JOIN_SKIP.load(Ordering::Relaxed);
    while skip != 0 {
        match JOIN_SKIP.compare_exchange_weak(skip, skip - 1, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return 0,
            Err(current) => skip = current,
        }
    }
    let started = now_qpc();
    let deadline = call!(p, tick_millis) + 20;
    while COMPLETED.load(Ordering::Acquire) < previous {
        let now = call!(p, tick_millis);
        if now >= deadline {
            JOIN_SKIP.store(120, Ordering::Relaxed);
            trace(
                "previous Present did not complete; joins paused",
                previous,
                NOT_READY,
            );
            break;
        }
        if call!(p, wait_completion, (deadline - now) as u32, 1) == 0 {
            return 0;
        }
    }
    now_qpc().saturating_sub(started)
}

#[derive(Default)]
struct Pacer {
    device: usize,
    vendor: u32,
    owner: u32,
}

static PACER: Mutex<Pacer> = Mutex::new(Pacer {
    device: 0,
    vendor: 0,
    owner: RSF_PACING_NONE,
});
static REFLEX_CONFIGURATION: Mutex<Option<(usize, u32, u32)>> = Mutex::new(None);

fn antilag(p: &Platform) -> Option<&'static AntiLag> {
    static CONTEXT: OnceLock<Option<&'static AntiLag>> = OnceLock::new();
    *CONTEXT.get_or_init(|| unsafe { call!(p, create_antilag).cast::<AntiLag>().as_ref() })
}

fn fsr(backend: u32) -> bool {
    matches!(backend, RSF_NFG_CPU_FSR3 | RSF_NFG_CPU_FSR4)
}

fn provider_status(p: &Platform) -> Option<rsf_d3d11_present_status> {
    let mut status: rsf_d3d11_present_status = unsafe { zeroed() };
    status.struct_size = size_of::<rsf_d3d11_present_status>() as u32;
    status.version = RSF_PRESENT_STATUS_VERSION;
    (call!(p, status, &mut status) == OK).then_some(status)
}

fn host_status(p: &Platform, host: Handle) -> Option<rsf_streamline_latency_status> {
    if host.is_null() {
        return None;
    }
    let mut status: rsf_streamline_latency_status = unsafe { zeroed() };
    status.struct_size = size_of::<rsf_streamline_latency_status>() as u32;
    (call!(p, host_status, host, &mut status) == OK).then_some(status)
}

struct Selection {
    owner: u32,
    denied: bool,
    anti_paced: bool,
}

fn select_pacer(p: &Platform, id: u64, mode: u32, limit: u32) -> Selection {
    let device = call!(p, device);
    if device.is_null() {
        lock(&PACER).owner = RSF_PACING_NONE;
        return Selection {
            owner: RSF_PACING_NONE,
            denied: false,
            anti_paced: false,
        };
    }
    let snapshot = provider_status(p);
    let state = snapshot
        .as_ref()
        .map_or_else(|| unsafe { zeroed() }, |s| s.state);
    let backend = snapshot.map_or_else(|| call!(p, backend), |s| s.backend);
    let api = if GRAPHICS_API.load(Ordering::Relaxed) == 12 || fsr(backend) {
        12
    } else {
        11
    };
    let host = call!(p, host);
    let reflex_available = host_status(p, host)
        .is_some_and(|s| s.low_latency_available != 0 && s.profile != RSF_SL_PROFILE_PCL);
    let anti = {
        let mut pacer = lock(&PACER);
        if pacer.device != device as usize {
            pacer.device = device as usize;
            pacer.vendor = call!(p, adapter_vendor, device);
        }
        antilag(p).filter(|anti| anti.bind(api, device, pacer.vendor))
    };
    let chosen = rsf_core_latency_pacing_owner(
        state.pacing_owner,
        u32::from(reflex_available),
        u32::from(anti.is_some_and(|anti| anti.state().available != 0)),
        RSF_PACING_ANTILAG2,
    );
    let denied = chosen != RSF_PACING_NONE && rsf_native_latency_claim(D3D_CLAIM) == 0;
    let mut owner = if denied { RSF_PACING_NONE } else { chosen };
    let other = if denied {
        RSF_PACING_REFLEX
    } else if owner == RSF_PACING_ANTILAG2 {
        RSF_PACING_NONE
    } else {
        owner
    };

    // Anti-Lag Update can delay the CPU. Its context borrows the driver while
    // the older frame's marker takes a separate shared borrow.
    let anti_paced = anti.is_some_and(|anti| anti.pace(id, mode, limit, other) != 0);
    if owner == RSF_PACING_ANTILAG2 && anti.is_none_or(|anti| anti.state().available == 0) {
        owner = RSF_PACING_NONE;
    }
    lock(&PACER).owner = owner;

    // DLSS-G configures its shared Reflex host at Prepare using this frame's
    // eligibility. Other providers use the last effective presentation ratio.
    let mut configured = lock(&REFLEX_CONFIGURATION);
    if reflex_available && backend != RSF_NFG_CPU_DLSS {
        let wanted = if owner == RSF_PACING_REFLEX {
            let presents = if state.effective_mode == RSF_FG_FIXED {
                state.effective_generated_frames.saturating_add(1)
            } else {
                1
            };
            (host as usize, mode, limit / presents)
        } else {
            (host as usize, RSF_REFLEX_OFF, 0)
        };
        if *configured != Some(wanted) {
            *configured = (call!(p, host_reflex, host, wanted.1, wanted.2) == OK).then_some(wanted);
        }
    } else {
        *configured = None;
    }
    Selection {
        owner,
        denied,
        anti_paced,
    }
}

pub(crate) fn pacing_status(p: &Platform, status: &mut Status) {
    let owner = lock(&PACER).owner;
    let update = match owner {
        RSF_PACING_NONE => Some((0, RSF_REFLEX_OFF, 0)),
        RSF_PACING_REFLEX => host_status(p, call!(p, host))
            .map(|s| (s.low_latency_available, s.reflex_mode, s.sleep_calls)),
        RSF_PACING_XELL => provider_status(p).map(|s| {
            (
                s.state.low_latency_available,
                s.state.effective_reflex,
                s.state.latency_paced_frames,
            )
        }),
        RSF_PACING_ANTILAG2 => antilag(p)
            .map(AntiLag::state)
            .filter(|s| s.available != 0)
            .map(|s| {
                (
                    1,
                    if s.enabled != 0 {
                        RSF_REFLEX_ON
                    } else {
                        RSF_REFLEX_OFF
                    },
                    s.updates,
                )
            }),
        _ => None,
    };
    if let Some((available, mode, frames)) = update {
        status.pacing_owner = owner;
        status.low_latency_available = available;
        status.effective_reflex = mode;
        status.latency_paced_frames = frames;
    }
}

fn pace_frame(p: &Platform, source: Source, prospective: Option<Reservation>) {
    let Some(claimed) = lock(&LEDGER).claim_pace(source) else {
        return;
    };
    let threaded = THREADED.load(Ordering::Relaxed);
    let joined = if prospective.is_some() {
        let joining = now_qpc();
        let (refusal, current) = loop {
            let current = source.reservation();
            match current {
                None => break (1, None),
                Some(r) if r.state != RSF_NATIVE_CPU_RESERVATION_OPEN => break (2, current),
                Some(r)
                    if r.predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_NONE
                        || r.predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED as i32 =>
                {
                    break (0, current);
                }
                Some(r) if r.predecessor_present != RSF_NATIVE_CPU_DEPENDENCY_PENDING as i32 => {
                    break (4, current);
                }
                _ => {
                    // Cancellation is recorded by the dispatcher independently
                    // of this callback. The callback owns this token until it returns.
                    if call!(p, wait_completion, 1, 0) == 0 {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }
        };
        let ticks = now_qpc().saturating_sub(joining);
        if refusal != 0 {
            if let Some(frame) = lock(&LEDGER).exact(source) {
                frame.failed = true;
                frame.join_ticks = ticks;
                frame.pacing_finished = true;
            }
            call!(p, abort, source.id);
            trace(
                "reserved source predecessor Present unavailable",
                source.id,
                NOT_READY,
            );
            failure(
                Failure::PaceDependency,
                source,
                NOT_READY,
                refusal,
                0,
                None,
                current.as_ref(),
            );
            return;
        }
        ticks
    } else if !threaded && !call!(p, host).is_null() {
        join_previous_present(p, claimed.previous)
    } else {
        0
    };

    let started = now_qpc();
    let options = requested();
    let generation = claimed.provider_generation;
    let selection = (generation == call!(p, generation))
        .then(|| select_pacer(p, source.id, options.reflex_mode, options.frame_limit_us));
    let actual_pacer = selection.as_ref().is_some_and(|s| {
        matches!(s.owner, RSF_PACING_REFLEX | RSF_PACING_XELL)
            || (s.owner == RSF_PACING_ANTILAG2 && s.anti_paced)
    });
    // No ledger or provider-settings lock spans this call. The presentation
    // bridge owns a read lease for the duration of the vendor's sleep.
    let result = selection
        .as_ref()
        .filter(|s| !s.denied)
        .map_or(NOT_READY, |s| {
            call!(p, pace, source.id, s.owner, generation)
        });
    let finished = now_qpc();
    let mut withdrawn = false;
    if let Some(frame) = lock(&LEDGER).exact(source) {
        frame.pacing_finished = true;
        withdrawn = frame.failed;
        frame.paced = !withdrawn && result == OK && (prospective.is_none() || actual_pacer);
        frame.failed |= result != OK;
        frame.reflex_ticks = finished.saturating_sub(started);
        frame.ready_qpc = finished;
        if !threaded || prospective.is_some() {
            frame.join_ticks = joined;
        }
    }
    if prospective.is_none() && result == OK {
        record_pacing(Some(source.session), source.id, started, finished);
    }
    trace("sleep at engine pacing boundary", source.id, result);
    if prospective.is_some() && (withdrawn || result != OK) {
        call!(p, abort, source.id);
    }
    let denied = selection.as_ref().is_some_and(|s| s.denied);
    let detail = selection.map_or(RSF_PACING_NONE, |s| s.owner) | (u32::from(denied) << 16);
    if prospective.is_some() && (!actual_pacer || denied) {
        failure(
            Failure::PaceSelection,
            source,
            result,
            detail | (u32::from(actual_pacer) << 17),
            generation,
            None,
            prospective.as_ref(),
        );
    }
    if result != OK {
        let source = if prospective.is_some() {
            source
        } else {
            Source {
                session: 0,
                ..source
            }
        };
        failure(
            Failure::PaceCall,
            source,
            result,
            detail,
            generation,
            None,
            prospective.as_ref(),
        );
    }
}

fn withdraw_reserved(p: &Platform, session: u64) {
    let mut withdrawn = [0; CAPACITY];
    for (out, frame) in withdrawn.iter_mut().zip(&mut lock(&LEDGER).frames) {
        if frame.prospective
            && frame.session == session
            && !frame.failed
            && frame.source().reservation().is_none_or(|r| {
                matches!(
                    r.state,
                    RSF_NATIVE_CPU_RESERVATION_ABORTED | RSF_NATIVE_CPU_RESERVATION_UNBOUND
                )
            })
        {
            frame.failed = true;
            if !frame.render_started
                && !frame.render_starting
                && !frame.submitted
                && !frame.pacing_in_flight()
            {
                *out = frame.id;
            }
        }
    }
    for id in withdrawn.into_iter().filter(|id| *id != 0) {
        call!(p, abort, id);
    }
}

fn reserve(p: &Platform, source: Source) {
    let Some(reservation) = source
        .reservation()
        .filter(|r| r.state == RSF_NATIVE_CPU_RESERVATION_OPEN)
    else {
        return;
    };
    let generation = call!(p, generation);
    {
        let mut ledger = lock(&LEDGER);
        let frame = ledger.slot(source.id);
        if frame.source() == source
            || frame.pacing_in_flight()
            || (frame.id != 0
                && !frame.ended
                && !(frame.failed && !frame.render_started && !frame.submitted))
        {
            return;
        }
        *frame = Frame {
            id: source.id,
            session: source.session,
            prospective: true,
            previous: reservation.predecessor_source_id,
            provider_generation: generation,
            ..Frame::default()
        };
    }
    let acquired = frame_acquire(p, source);
    let cancelled = lock(&LEDGER).exact(source).is_some_and(|frame| {
        frame.failed |= acquired != OK;
        frame.failed
    });
    if cancelled {
        call!(p, abort, source.id);
    } else {
        pace_frame(p, source, Some(reservation));
    }
}

fn begin(p: &Platform, event: &Event, source: Source) {
    let joined_at = now_qpc();
    let generation = call!(p, generation);
    let reserved = lock(&LEDGER)
        .exact(source)
        .is_some_and(|frame| frame.prospective);
    let result = if reserved {
        OK
    } else {
        frame_acquire(p, source)
    };
    if let Some(latency) = host_status(p, call!(p, host)) {
        rsf_native_cpu_ping_message(latency.pcl_message_id);
    }
    let ready = now_qpc();
    trace(
        "reserve frame before BeginFrame dispatch",
        source.id,
        result,
    );
    let mut ledger = lock(&LEDGER);
    let previous = ledger.last_begun;
    ledger.last_begun = source;
    if reserved {
        let adopted = source.reservation().is_some_and(|r| {
            r.state == RSF_NATIVE_CPU_RESERVATION_ADOPTED && r.poll_begin_qpc != 0
        });
        let pacing = ledger.exact(source).and_then(|frame| {
            frame.failed |= !adopted;
            let began = frame.ready_qpc.saturating_sub(frame.reflex_ticks);
            (frame.paced && !frame.failed && began != 0).then_some((began, frame.ready_qpc))
        });
        drop(ledger);
        if !adopted {
            call!(p, abort, source.id);
        }
        if let Some((began, ready)) = pacing {
            record_pacing(Some(source.session), source.id, began, ready);
        }
    } else {
        let frame = ledger.slot(source.id);
        // A different source at this index is at least one ring old. Ended
        // and abandoned records can both be reclaimed.
        if frame.source() != source || frame.ended {
            *frame = Frame {
                id: source.id,
                session: source.session,
                failed: result != OK,
                provider_generation: generation,
                join_ticks: joined_at.saturating_sub(event.timestamp_qpc),
                ready_qpc: ready,
                previous: if previous.session == source.session && previous.id < source.id {
                    previous.id
                } else {
                    0
                },
                ..Frame::default()
            };
        }
    }
}

fn end(p: &Platform, event: &Event, source: Source) {
    let required = if rendered_source() {
        (1 << RSF_GAME_CPU_FRAME_BEGIN) | (1 << RSF_GAME_CPU_SIMULATION_END)
    } else {
        SIM_INTERVAL
    };
    let aborted = {
        let mut ledger = lock(&LEDGER);
        let Some(frame) = ledger.exact(source) else {
            return;
        };
        let aborted = (frame.prospective && frame.failed)
            || event.flags & RSF_GAME_CPU_RENDER_SKIPPED != 0
            || source
                .facts()
                .is_some_and(|f| f.stage_mask & required != required);
        frame.ended = true;
        frame.expected_count = event.render_expected;
        frame.failed |= aborted;
        aborted
    };
    if aborted {
        call!(p, abort, source.id);
        complete(p, source.id);
    }
    audit(p, source);
    report_latency_window(p);
}

fn route_receipt(p: &Platform, receipt: &rsf_game_cpu_receipt_event) {
    let event = &receipt.header;
    if event.stage != RSF_GAME_CPU_INPUT_EVENT
        || event.source_frame_id != 0
        || (event.input_kind == 0 && rsf_native_cpu_is_ping_message(event.message_id) == 0)
    {
        return;
    }
    let generation = call!(p, generation);
    let (selected, binding) = {
        let mut ledger = lock(&LEDGER);
        let binding = ledger.binding;
        let thread = if binding.key == 0 {
            0
        } else {
            call!(p, owned_window_thread, binding.key)
        };
        let target = binding.session == event.session_id
            && thread != 0
            && thread == binding.thread
            && if receipt.window_key != 0 {
                receipt.window_key == binding.key
            } else {
                call!(p, thread_id) == binding.thread
            };
        let mut candidates: [Owner; CAPACITY] = unsafe { zeroed() };
        if target {
            for (candidate, frame) in candidates.iter_mut().zip(&ledger.frames) {
                if frame.id == 0
                    || frame.session != event.session_id
                    || !frame.pacing_finished
                    || frame.failed
                    || frame.ended
                    || frame.render_started
                    || frame.submitted
                {
                    continue;
                }
                let Some(facts) = frame.source().facts().filter(|f| {
                    f.failed == 0
                        && f.ended == 0
                        && f.stage_mask == ACTUAL_INPUT
                        && f.flags & (RSF_GAME_CPU_INPUT_SUBSTITUTE | RSF_GAME_CPU_DERIVED_BOUNDARY)
                            == 0
                }) else {
                    continue;
                };
                *candidate = Owner {
                    source: frame.id,
                    session: frame.session,
                    generation: frame.provider_generation,
                    ready: frame.ready_qpc,
                    simulation_begin: facts.timestamps_qpc[RSF_GAME_CPU_SIMULATION_BEGIN as usize],
                    eligible: 1,
                };
            }
        }
        let selected = call!(
            p,
            receipt_owner,
            candidates.as_ptr(),
            candidates.len(),
            event.session_id,
            generation,
            event.timestamp_qpc
        );
        if selected == 0 {
            ledger.routes.unmatched += 1;
            return;
        }
        if receipt.window_key != 0 {
            ledger.routes.matched_window += 1;
        } else {
            ledger.routes.matched_thread += 1;
        }
        ledger.routes.last_receipt = receipt.receipt_id;
        ledger.routes.last_source = selected;
        (selected, binding)
    };
    // The provider operation verifies the generation and uses its existing
    // token. Attribute the result only to the binding that selected it.
    let result = call!(
        p,
        receipt,
        generation,
        selected,
        event.input_kind,
        event.message_id
    );
    let mut ledger = lock(&LEDGER);
    if ledger.binding.epoch == binding.epoch {
        if result == OK {
            ledger.routes.routed += 1;
        } else {
            ledger.routes.failed += 1;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_cpu(event: *const Event, p: &Platform) {
    if event.is_null() || unsafe { event.cast::<u32>().read() } < size_of::<Event>() as u32 {
        return;
    }
    let event_ref = unsafe { &*event };
    if event_ref.event_version == RSF_GAME_CPU_EVENT_VERSION_RECEIPT {
        if event_ref.struct_size >= size_of::<rsf_game_cpu_receipt_event>() as u32 {
            route_receipt(p, unsafe { &*event.cast() });
        }
        return;
    }
    let stage = event_ref.stage;
    if (event_ref.flags & (RSF_GAME_CPU_INPUT_SUBSTITUTE | RSF_GAME_CPU_DERIVED_BOUNDARY) != 0
        && matches!(
            stage,
            RSF_GAME_CPU_INPUT_SAMPLE | RSF_GAME_CPU_SIMULATION_BEGIN | RSF_GAME_CPU_SIMULATION_END
        ))
        || call!(p, has_owner) == 0
    {
        return;
    }
    let source = Source::from_event(event_ref);
    let rendered = rendered_source();
    if !rendered
        && matches!(
            stage,
            RSF_GAME_CPU_SOURCE_RESERVE
                | RSF_GAME_CPU_SOURCE_ABORT
                | RSF_GAME_CPU_FRAME_BEGIN
                | RSF_GAME_CPU_FRAME_END
        )
    {
        withdraw_reserved(p, source.session);
    }
    if (RSF_GAME_CPU_SOURCE_RESERVE..=RSF_GAME_CPU_SOURCE_ABORT).contains(&stage) {
        if !rendered && stage == RSF_GAME_CPU_SOURCE_RESERVE {
            reserve(p, source);
        }
        return;
    }
    match stage {
        RSF_GAME_CPU_FRAME_BEGIN => begin(p, event_ref, source),
        RSF_GAME_CPU_FRAME_END => end(p, event_ref, source),
        _ if !rendered => dispatch_boundary(p, event_ref, source),
        _ => {}
    }
}

fn dispatch_boundary(p: &Platform, event: &Event, source: Source) {
    let generation = call!(p, generation);
    let Some(frame) = lock(&LEDGER).exact(source).map(|frame| {
        if frame.prospective {
            frame.failed |= frame.provider_generation != generation;
        }
        *frame
    }) else {
        return;
    };
    let bound = if frame.prospective {
        let Some(reservation) = source
            .reservation()
            .filter(|r| !frame.failed && r.state == RSF_NATIVE_CPU_RESERVATION_BOUND)
        else {
            return;
        };
        Some(reservation)
    } else {
        None
    };
    let input_keyed = event.event_version == RSF_GAME_CPU_EVENT_VERSION
        && event.flags & RSF_GAME_CPU_INPUT_KEYED != 0;
    match event.stage {
        RSF_GAME_CPU_PACING if !input_keyed => pace_frame(p, source, None),
        // Input sample is the legacy keyed pacing boundary. A reservation
        // already ran the pacer before its actual poll.
        RSF_GAME_CPU_INPUT_SAMPLE if bound.is_none() && input_keyed => pace_frame(p, source, None),
        RSF_GAME_CPU_SIMULATION_BEGIN if bound.is_none_or(|r| r.poll_begin_qpc != 0) => {
            trace(
                "actual simulation begin",
                source.id,
                frame_marker(p, frame, RSF_NFG_CPU_SIM_START).0,
            );
        }
        RSF_GAME_CPU_SIMULATION_END => {
            trace(
                "simulation end",
                source.id,
                frame_marker(p, frame, RSF_NFG_CPU_SIM_END).0,
            );
        }
        RSF_GAME_CPU_INPUT_EVENT
            if frame.provider_generation == generation
                && (!frame.prospective
                    || event.flags
                        & (RSF_GAME_CPU_INPUT_UNFRAMED | RSF_GAME_CPU_WINDOW_MESSAGE)
                        == 0) =>
        {
            trace(
                "input received",
                source.id,
                call!(p, input, source.id, event.input_kind, event.message_id),
            );
        }
        _ => {}
    }
}

fn start_render_marker(p: &Platform, id: u64) {
    let generation = call!(p, generation);
    let frame = {
        let mut ledger = lock(&LEDGER);
        let Some(frame) = ledger.frame(id) else {
            return;
        };
        if frame.failed {
            let source = frame.source();
            drop(ledger);
            failure(Failure::RenderOwner, source, NOT_READY, 1, 0, None, None);
            return;
        }
        if frame.render_started || frame.render_starting {
            return;
        }
        if rendered_source() {
            frame.render_started = true;
            return;
        }
        frame.render_starting = true;
        *frame
    };
    let source = frame.source();
    // A pending attempt cannot authorize an end or Present marker.
    let finish = |accepted, failed| {
        if let Some(frame) = lock(&LEDGER).exact(source) {
            frame.render_starting = false;
            frame.render_started = accepted;
            frame.failed |= failed;
        }
    };
    let facts = source.facts();
    let Some(f) = facts.filter(|f| f.failed == 0) else {
        finish(false, false);
        failure(
            Failure::RenderCpu,
            source,
            NOT_READY,
            0,
            generation,
            facts.as_ref(),
            None,
        );
        return;
    };
    let reservation = if frame.prospective {
        source.reservation()
    } else {
        None
    };
    if frame.prospective {
        let bound = reservation
            .is_some_and(|r| r.state == RSF_NATIVE_CPU_RESERVATION_BOUND && r.poll_begin_qpc != 0);
        let post_update = frame.submitted
            && frame.scene_begins != 0
            && frame.first_submission != 0
            && frame.first_view != 0
            && frame.provider_generation == generation;
        let substitute = f.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE != 0;
        let missing = f.stage_mask & ACTUAL_INPUT != ACTUAL_INPUT;
        if !bound || !post_update || substitute || missing {
            finish(false, true);
            let detail = u32::from(!bound)
                | (u32::from(!post_update) << 1)
                | (u32::from(substitute) << 2)
                | (u32::from(missing) << 3);
            failure(
                Failure::RenderProspective,
                source,
                NOT_READY,
                detail,
                generation,
                Some(&f),
                reservation.as_ref(),
            );
            return;
        }
    }
    // Only the real simulation interval authorizes vendor submission timing.
    if f.stage_mask & SIM_INTERVAL != SIM_INTERVAL
        || f.marker_mask & 3 != 3
        || f.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE != 0
    {
        finish(false, false);
        failure(
            Failure::RenderInput,
            source,
            NOT_READY,
            0,
            generation,
            Some(&f),
            None,
        );
        return;
    }
    let (result, marked) = frame_marker(p, frame, RSF_NFG_CPU_SUBMIT_START);
    finish(marked, result != OK);
    trace("actual graphics submit begin", id, result);
    if !marked || result != OK {
        failure(
            Failure::RenderProvider,
            source,
            result,
            u32::from(marked),
            generation,
            Some(&f),
            reservation.as_ref(),
        );
    }
}

fn end_render_marker(p: &Platform, id: u64, stage: &str) {
    let frame = {
        let mut ledger = lock(&LEDGER);
        let Some(frame) = ledger.frame(id) else {
            return;
        };
        if frame.failed || !frame.render_started || frame.submit_ended || frame.submit_ending {
            return;
        }
        frame.submit_ending = true;
        *frame
    };
    if !rendered_source()
        && !frame.source().facts().is_some_and(|f| {
            let required = marker_prerequisites(f.flags, RSF_NFG_CPU_SUBMIT_END);
            f.failed == 0 && f.marker_mask & required == required
        })
    {
        if let Some(frame) = lock(&LEDGER).exact(frame.source()) {
            frame.submit_ending = false;
        }
        return;
    }
    let (result, marked) = frame_marker(p, frame, RSF_NFG_CPU_SUBMIT_END);
    if let Some(frame) = lock(&LEDGER).exact(frame.source()) {
        frame.submit_ending = false;
        frame.submit_ended = marked;
        frame.failed |= result != OK;
    }
    trace(stage, id, result);
}

pub(crate) fn latency_present(p: &Platform, id: u64, chain: Handle, begin: bool) {
    if id == 0 {
        return;
    }
    if begin {
        start_render_marker(p, id);
        let threaded = THREADED.load(Ordering::Relaxed);
        if !threaded {
            end_render_marker(p, id, "render submission end before present");
        }
        let frame = {
            let mut ledger = lock(&LEDGER);
            let Some(frame) = ledger.frame(id) else {
                return;
            };
            if frame.failed
                || !frame.render_started
                || frame.present_started
                || frame.present_starting
            {
                return;
            }
            frame.present_starting = true;
            *frame
        };
        let rendered = rendered_source();
        let facts = frame.source().facts();
        let keyed = facts.is_some_and(|f| f.flags & RSF_GAME_CPU_INPUT_KEYED != 0);
        if ((keyed || !threaded) && !frame.submit_ended)
            || (!rendered
                && !facts.is_some_and(|f| {
                    let required = marker_prerequisites(f.flags, RSF_NFG_CPU_PRESENT_START);
                    f.failed == 0 && f.marker_mask & required == required
                }))
        {
            if let Some(frame) = lock(&LEDGER).exact(frame.source()) {
                frame.present_starting = false;
            }
            return;
        }
        if !rendered && let Some(anti) = antilag(p) {
            anti.end_render(id, chain, u32::from(fsr(call!(p, backend))));
        }
        let (result, marked) = frame_marker(p, frame, RSF_NFG_CPU_PRESENT_START);
        if let Some(frame) = lock(&LEDGER).exact(frame.source()) {
            frame.present_starting = false;
            frame.present_started = marked;
            frame.failed |= result != OK;
        }
        trace("present begin", id, result);
    } else {
        let frame = lock(&LEDGER).frame(id).and_then(|f| {
            if f.failed || !f.present_started || f.present_ended || f.present_ending {
                return None;
            }
            f.present_ending = true;
            Some(*f)
        });
        if let Some(frame) = frame {
            let (result, marked) = frame_marker(p, frame, RSF_NFG_CPU_PRESENT_END);
            if let Some(frame) = lock(&LEDGER).exact(frame.source()) {
                frame.present_ending = false;
                frame.present_ended = marked;
                frame.failed |= result != OK;
            }
            trace("present end", id, result);
        }
        complete(p, id);
    }
}

fn has_pass_size(pass: *const Pass) -> bool {
    !pass.is_null() && unsafe { pass.cast::<u32>().read() } >= size_of::<Pass>() as u32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_frame(pass: *const Pass, begin: u32, p: &Platform) {
    if !has_pass_size(pass) {
        return;
    }
    let pass = unsafe { &*pass };
    let source = Source::from_pass(pass);
    if pass.role != RSF_GAME_RENDER_FRAME || source.id == 0 || call!(p, has_owner) == 0 {
        return;
    }
    {
        let mut ledger = lock(&LEDGER);
        if ledger.exact(source).is_none() {
            return;
        }
        if begin != 0 && rendered_source() {
            ledger.rendered_begin(
                pass,
                call!(p, generation),
                now_qpc(),
                call!(p, qpc_frequency),
            );
        } else if let Some(frame) = ledger.exact(source) {
            if begin != 0 {
                frame.rhi_begins += 1;
            } else {
                frame.rhi_ends += 1;
            }
        }
    }
    if begin != 0 {
        // Queued BeginFrame can precede CPU pacing. Open the vendor interval
        // at the real submission so the empty queue is outside GPU timing.
        RHI_SOURCE.set(source);
        return;
    }
    end_render_marker(p, source.id, "full RHI frame end");
    if progress(p, source.id).0 {
        call!(p, abort, source.id);
    }
    complete(p, source.id);
    if RHI_SOURCE.get() == source {
        RHI_SOURCE.set(Source::default());
    }
    audit(p, source);
}

fn watch(p: &Platform, pass: &Pass, begin: bool) {
    if !begin {
        hitch::close(Source::from_pass(pass), pass.submission_id);
        return;
    }
    if requested().debug == 0 {
        return;
    }
    let now = call!(p, tick_millis);
    let (screen, since) = SHOWN.get();
    if screen != pass.screen {
        SHOWN.set((pass.screen, now));
        return;
    }
    let mut record: rsf_frame_record = unsafe { zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.screen = screen;
    if now.saturating_sub(since) >= 3000 && call!(p, allows_fg, &record) != 0 {
        hitch::open(p, Source::from_pass(pass), pass.submission_id, now_qpc());
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_submission(pass: *const Pass, begin: u32, p: &Platform) {
    if !has_pass_size(pass) {
        return;
    }
    let pass = unsafe { &*pass };
    let source = Source::from_pass(pass);
    let required = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
    if source.id == 0 || pass.flags & required != required || call!(p, has_owner) == 0 {
        return;
    }
    watch(p, pass, begin != 0);
    let now = now_qpc();
    let mut dropped = [0; CAPACITY];
    let start = {
        let mut ledger = lock(&LEDGER);
        let Some(frame) = ledger.exact(source) else {
            return;
        };
        if begin == 0 {
            frame.scene_ends += 1;
            frame.submit_end = now;
            frame.render_complete = true;
            return;
        }
        if frame.scene_begins == 0 {
            frame.first_submission = pass.submission_id;
            frame.first_view = pass.view_key;
        }
        frame.scene_begins += 1;
        frame.last_submission = pass.submission_id;
        frame.last_view = pass.view_key;
        frame.screen = pass.screen;
        frame.native_frame = pass.native_frame as u32;
        let start = !frame.failed && !frame.submitted;
        if start {
            frame.submitted = true;
            frame.submit_begin = now;
        }
        // Queued older frames may release CPU tokens once their reporting
        // ended without opening RHI submission. Never retire a sleeper.
        for (id, frame) in dropped.iter_mut().zip(&mut ledger.frames) {
            if frame.id != 0
                && frame.session == source.session
                && frame.id < source.id
                && frame.ended
                && !frame.submitted
                && !frame.render_started
                && !frame.render_starting
                && !frame.failed
                && !frame.pacing_in_flight()
            {
                frame.failed = true;
                *id = frame.id;
            }
        }
        start
    };
    for id in dropped.into_iter().filter(|id| *id != 0) {
        call!(p, abort, id);
    }
    if start {
        start_render_marker(p, source.id);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_fg_reflex_ordering_set(threaded: u32) {
    THREADED.store(threaded != 0, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_latency_graphics(api: u32) {
    if matches!(api, 11 | 12) {
        GRAPHICS_API.store(api, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_latency_render(id: u64, begin: u32, p: &Platform) {
    if begin != 0 {
        start_render_marker(p, id);
    } else {
        end_render_marker(p, id, "render submission end");
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_latency_present(id: u64, chain: Handle, begin: u32, p: &Platform) {
    latency_present(p, id, chain, begin != 0);
}

unsafe fn has_capacity<T>(value: *const T) -> bool {
    !value.is_null() && unsafe { value.cast::<u32>().read() } >= size_of::<T>() as u32
}

fn empty_routes() -> Routes {
    Routes {
        struct_size: size_of::<Routes>() as u32,
        version: 1,
        ..Routes::default()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_window_bind(
    binding: *const Window,
    p: &Platform,
) -> i32 {
    if !unsafe { has_capacity(binding) } {
        return INVALID;
    }
    let binding = unsafe { &*binding };
    if binding.session_id == 0 || binding.window_key == 0 {
        return INVALID;
    }
    if binding.version != 1 {
        return RSF_NFG_CPU_ABI_MISMATCH;
    }
    let thread = call!(p, owned_window_thread, binding.window_key);
    if thread == 0 {
        return INVALID;
    }
    let mut ledger = lock(&LEDGER);
    let mut epoch = ledger.binding.epoch;
    if (
        ledger.binding.session,
        ledger.binding.key,
        ledger.binding.thread,
    ) != (binding.session_id, binding.window_key, thread)
    {
        ledger.routes = empty_routes();
        epoch = epoch.wrapping_add(1);
    }
    ledger.binding = Binding {
        key: binding.window_key,
        session: binding.session_id,
        thread,
        epoch,
    };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_input_routes_get(out: *mut Routes) -> i32 {
    if !unsafe { has_capacity(out) } || unsafe { (*out).version } != 1 {
        return 0;
    }
    unsafe { out.write(lock(&LEDGER).routes) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_frame_get(out: *mut LatencyFrame) -> i32 {
    if !unsafe { has_capacity(out) } || unsafe { (*out).version } != 1 {
        return 0;
    }
    let out = unsafe { &mut *out };
    let Some(frame) = lock(&LEDGER).frame(out.source).copied() else {
        return 0;
    };
    out.generation = frame.provider_generation;
    out.ready_qpc = frame.ready_qpc;
    out.paced = u32::from(frame.paced);
    out.failed = u32::from(frame.failed);
    out.completed = u32::from(COMPLETED.load(Ordering::Acquire) >= frame.id);
    1
}

fn registered_ping(message: u32) -> bool {
    (0xc000..=0xffff).contains(&message)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_status_get(
    out: *mut LatencyStatus,
    p: &Platform,
) -> i32 {
    if !unsafe { has_capacity(out) } || unsafe { (*out).version } != 1 {
        return 0;
    }
    let mut out_value: LatencyStatus = unsafe { zeroed() };
    out_value.struct_size = size_of::<LatencyStatus>() as u32;
    out_value.version = 1;
    if !rendered_source() {
        let mut status: Status = unsafe { zeroed() };
        pacing_status(p, &mut status);
        out_value.owner = status.pacing_owner;
        out_value.available = status.low_latency_available;
        out_value.effective_mode = status.effective_reflex;
        out_value.paced_frames = status.latency_paced_frames;
        if let Some(sdk) = host_status(p, call!(p, host)) {
            out_value.pcl_available = u32::from(registered_ping(sdk.pcl_message_id));
            out_value.report_available = sdk.latency_report_available;
            out_value.marker_calls = sdk.marker_calls;
        }
        out_value.events = counters();
        out_value.derived_boundaries =
            DERIVED.load(Ordering::Relaxed) + out_value.events.derived_frame_ends;
    }
    unsafe { out.write(out_value) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_latency_ping(window: Handle, p: &Platform) -> i32 {
    i32::from(
        !window.is_null()
            && host_status(p, call!(p, host)).is_some_and(|sdk| {
                registered_ping(sdk.pcl_message_id)
                    && call!(p, post_ping, window, sdk.pcl_message_id) != 0
            }),
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_latency_retire(session: u64, through: u64, p: &Platform) {
    // The caller reports older sources with no submitted GPU work.
    let host = call!(p, host);
    if !host.is_null() {
        call!(p, host_retire, host, through);
    }
    for frame in &mut lock(&LEDGER).frames {
        if frame.session == session && frame.id != 0 && frame.id <= through && !frame.render_started
        {
            frame.failed = true;
        }
    }
    complete(p, through);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_latency_end_session(session: u64, p: &Platform) {
    // All producers and leased graphics work have drained. The CPU dispatcher
    // owns admission and can refuse shutdown of a different session.
    if rsf_native_cpu_release_session(session) == 0 {
        return;
    }
    hitch::end_session(session);
    let host = call!(p, host);
    if !host.is_null() {
        call!(p, host_retire, host, u64::MAX);
        call!(p, host_reset, host);
        call!(p, host_reflex, host, RSF_REFLEX_OFF, 0);
    }
    {
        let mut ledger = lock(&LEDGER);
        let epoch = ledger.binding.epoch.wrapping_add(1);
        *ledger = Ledger::new();
        ledger.binding.epoch = epoch;
    }
    if let Some(anti) = antilag(p) {
        anti.reset_sources();
    }
    lock(&PACER).owner = RSF_PACING_NONE;
    *lock(&REFLEX_CONFIGURATION) = None;
    COMPLETED.store(0, Ordering::Release);
    JOIN_SKIP.store(0, Ordering::Relaxed);
    DERIVED.store(0, Ordering::Relaxed);
    rsf_native_latency_release_claim(D3D_CLAIM);
}
