// SPDX-License-Identifier: MIT
//! Direct3D pacing and submission ownership. The dispatcher in native_cpu
//! owns producer facts and reservations. This owner tracks attempts and the
//! provider generation that consumed those facts.
use crate::input_receipts::{self, Owner};
use crate::latency_antilag::Context as AntiLag;
use crate::latency_lease::{
    pacing_owner, rsf_native_latency_claim, rsf_native_latency_release_claim,
};
use crate::native_cpu::{
    THROUGH_SIM_BEGIN, THROUGH_SIM_END, counters, frame_facts, mark_frame, marker_prerequisites,
    observed_boundary, owns_session, pacing_boundary, record_pacing, rendered_source, reservation,
    rsf_native_cpu_is_ping_message, rsf_native_cpu_ping_message, rsf_native_cpu_release_session,
};
use crate::native_fg::requested;
use crate::now_qpc;
use crate::sr_session::has_capacity;
use crate::sync::lock;
use crate::sys::rsf_native_latency_frame as LatencyFrame;
use crate::sys::*;
use crate::sys::{
    RSF_FG_FIXED, RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_PACING_XELL, RSF_REFLEX_OFF,
    rsf_fg_status as Status,
};
use crate::sys::{
    RSF_REFLEX_ON, rsf_native_input_routes as Routes, rsf_native_latency_status as LatencyStatus,
};
use rsf_game_sys::backend_result::OK;
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::zeroed;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

type Event = rsf_game_cpu_event;
type Pass = rsf_game_render_pass;
type Reservation = rsf_native_cpu_reservation;
type Facts = rsf_native_cpu_frame;
type Window = rsf_native_latency_window;
type Handle = *mut c_void;

const CAPACITY: usize = 128;
const D3D_CLAIM: u32 = 1;
const NOT_READY: i32 = -8;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
// The latency markers the presentation bridge takes, in frame order (rsf_d3d11_present_marker).
const SIM_START: u32 = 0;
const SIM_END: u32 = 1;
const SUBMIT_START: u32 = 2;
const SUBMIT_END: u32 = 3;
const PRESENT_START: u32 = 4;
const PRESENT_END: u32 = 5;
macro_rules! call {
    ($f:ident $(, $a:expr)* $(,)?) => {{
        #[allow(unused_unsafe)]
        let value = unsafe { leaf::$f($($a),*) };
        value
    }};
}

mod diagnostics;
#[cfg(feature = "fixture-leaves")]
mod fixture;
mod hitch;
pub(super) mod platform;
// Production calls the leaves in platform.rs; the fixture-leaves archive calls the FG CPU fixture's
// doubles with the same names.
#[cfg(feature = "fixture-leaves")]
use fixture::leaf;
#[cfg(not(feature = "fixture-leaves"))]
use platform::leaf;
mod win32;
use diagnostics::{Diagnostics, audit, failure, report_latency_window};
pub(crate) use diagnostics::{Failure, report_timing, sr_failure, stage, trace};

fn wait_completion(timeout: u32, pump: u32) -> i32 {
    leaf::FIXTURE_WAIT_COMPLETION.map_or_else(
        || win32::wait_completion(timeout, pump),
        |wait| unsafe { wait(timeout, pump) },
    )
}

fn owned_window_thread(key: u64) -> u32 {
    leaf::FIXTURE_OWNED_WINDOW_THREAD.map_or_else(
        || win32::owned_window_thread(key),
        |thread| unsafe { thread(key) },
    )
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Attempt {
    #[default]
    Idle,
    Pending,
    Done,
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
    pacing: Attempt,
    markers: [Attempt; 6],
    marker_failed: bool,
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

    fn marked(self, marker: u32) -> bool {
        self.markers[marker as usize] == Attempt::Done
    }

    fn attempted(self, marker: u32) -> bool {
        self.markers[marker as usize] != Attempt::Idle
    }

    fn unsubmitted_before(self, source: Source) -> bool {
        self.id != 0
            && self.id < source.id
            && self.session == source.session
            && !self.failed
            && !self.submitted
            && !self.attempted(SUBMIT_START)
            && !self.pacing_in_flight()
    }

    fn pacing_in_flight(self) -> bool {
        self.pacing == Attempt::Pending
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
        if frame.pacing != Attempt::Idle || frame.failed {
            return None;
        }
        frame.pacing = Attempt::Pending;
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

pub(crate) fn complete(source: u64) {
    COMPLETED.fetch_max(source, Ordering::AcqRel);
    win32::signal_completion();
}

pub(crate) fn progress(source: u64) -> (bool, bool) {
    let host = call!(host);
    if host.is_null() {
        (
            source != 0 && COMPLETED.load(Ordering::Acquire) < source,
            false,
        )
    } else {
        let live = !call!(host_token, host, source).is_null();
        (live, live && call!(host_presented, host, source) != 0)
    }
}

// Called by the fg12 owner, which unit tests do not reach.
#[cfg_attr(test, allow(dead_code))]
pub(super) fn retire_unsubmitted(session: u64, id: u64, abort_token: impl Fn(u64) -> i32) {
    let source = Source { session, id };
    for frame in &mut lock(&LEDGER).frames {
        // The Streamline abort only removes the token from its CPU ledger. Hold
        // admission across that operation so render and pacing cannot claim the source.
        if frame.unsubmitted_before(source)
            && frame.source().facts().is_some_and(|facts| facts.ended != 0)
            && abort_token(frame.id) == OK
        {
            frame.failed = true;
        }
    }
}

fn frame_acquire(source: Source) -> i32 {
    if source.id == 0 {
        return INVALID;
    }
    let result = call!(acquire, source.id);
    let host = call!(host);
    if result != NOT_READY || host.is_null() {
        return result;
    }
    let oldest = lock(&LEDGER)
        .frames
        .iter()
        .copied()
        .filter(|f| {
            f.unsubmitted_before(source) && f.source().facts().is_some_and(|facts| facts.ended != 0)
        })
        .min_by_key(|f| f.id);
    if let Some(oldest) = oldest
        && call!(host_abort, host, oldest.id) == OK
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
        return call!(acquire, source.id);
    }
    result
}

fn claim_marker(frame: &mut Frame, marker: u32) -> Option<Frame> {
    let attempt = &mut frame.markers[marker as usize];
    if frame.failed || frame.marker_failed || *attempt != Attempt::Idle {
        return None;
    }
    *attempt = Attempt::Pending;
    Some(*frame)
}

fn settle_marker(source: Source, marker: u32, marked: bool, marker_failed: bool) {
    if let Some(frame) = lock(&LEDGER).exact(source) {
        frame.markers[marker as usize] = if marked { Attempt::Done } else { Attempt::Idle };
        frame.marker_failed |= marker_failed;
        // Preserve the existing render-stage failure signal to FG admission.
        frame.failed |= marker_failed && marker >= SUBMIT_START;
    }
}

fn deliver_marker(frame: Frame, marker: u32) -> (i32, bool) {
    let (result, marked) = frame_marker(frame, marker);
    settle_marker(frame.source(), marker, marked, result != OK);
    (result, marked)
}

fn simulation_marker(source: Source, marker: u32, label: &str) {
    let frame = lock(&LEDGER)
        .exact(source)
        .and_then(|frame| claim_marker(frame, marker));
    if let Some(frame) = frame {
        trace(label, source.id, deliver_marker(frame, marker).0);
    }
}

fn frame_marker(frame: Frame, marker: u32) -> (i32, bool) {
    if rendered_source() {
        return (OK, true);
    }
    let generation = frame.provider_generation;
    if frame.failed || frame.pacing_in_flight() || generation != call!(generation) {
        return (NOT_READY, false);
    }
    let Some(facts) = frame.source().facts() else {
        return (NOT_READY, false);
    };
    let required = marker_prerequisites(facts.flags, marker);
    if frame
        .markers
        .iter()
        .enumerate()
        .any(|(index, attempt)| required & (1 << index) != 0 && *attempt != Attempt::Done)
    {
        return (NOT_READY, false);
    }
    match mark_frame(Some(frame.session), frame.id, marker) {
        1 => {
            let result = call!(marker, generation, frame.id, marker, 0);
            (result, result == OK)
        }
        0 => (OK, false),
        _ => (INVALID, false),
    }
}

// Drain only sent messages while joining an ordered Present. Posted input
// remains queued for the engine's actual input callback after pacing.
fn join_previous_present(previous: u64) -> u64 {
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
    let deadline = win32::tick_millis() + 20;
    while COMPLETED.load(Ordering::Acquire) < previous {
        let now = win32::tick_millis();
        if now >= deadline {
            JOIN_SKIP.store(120, Ordering::Relaxed);
            trace(
                "previous Present did not complete; joins paused",
                previous,
                NOT_READY,
            );
            break;
        }
        if wait_completion((deadline - now) as u32, 1) == 0 {
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

fn antilag() -> Option<&'static AntiLag> {
    static CONTEXT: OnceLock<Option<&'static AntiLag>> = OnceLock::new();
    *CONTEXT.get_or_init(|| unsafe { call!(create_antilag).cast::<AntiLag>().as_ref() })
}

fn fsr(backend: u32) -> bool {
    matches!(backend, RSF_FG_BACKEND_FSR3 | RSF_FG_BACKEND_FSR4)
}

fn provider_status() -> Option<rsf_d3d11_present_status> {
    let mut status: rsf_d3d11_present_status = unsafe { zeroed() };
    status.struct_size = size_of::<rsf_d3d11_present_status>() as u32;
    status.version = RSF_PRESENT_STATUS_VERSION;
    (call!(status, &mut status) == OK).then_some(status)
}

fn host_status(host: Handle) -> Option<rsf_streamline_latency_status> {
    if host.is_null() {
        return None;
    }
    let mut status: rsf_streamline_latency_status = unsafe { zeroed() };
    status.struct_size = size_of::<rsf_streamline_latency_status>() as u32;
    (call!(host_status, host, &mut status) == OK).then_some(status)
}

struct Selection {
    owner: u32,
    denied: bool,
    anti_paced: bool,
}

fn select_pacer(id: u64, mode: u32, limit: u32) -> Selection {
    let device = call!(device);
    if device.is_null() {
        lock(&PACER).owner = RSF_PACING_NONE;
        return Selection {
            owner: RSF_PACING_NONE,
            denied: false,
            anti_paced: false,
        };
    }
    let snapshot = provider_status();
    let state = snapshot
        .as_ref()
        .map_or_else(|| unsafe { zeroed() }, |s| s.state);
    let backend = snapshot.map_or_else(|| call!(backend), |s| s.backend);
    let api = if GRAPHICS_API.load(Ordering::Relaxed) == 12 || fsr(backend) {
        12
    } else {
        11
    };
    let host = call!(host);
    let reflex_available = host_status(host)
        .is_some_and(|s| s.low_latency_available != 0 && s.profile != RSF_SL_PROFILE_PCL);
    let anti = {
        let mut pacer = lock(&PACER);
        if pacer.device != device as usize {
            pacer.device = device as usize;
            pacer.vendor = call!(adapter_vendor, device);
        }
        antilag().filter(|anti| anti.bind(api, device, pacer.vendor))
    };
    let chosen = pacing_owner(
        state.pacing_owner,
        reflex_available,
        anti.is_some_and(|anti| anti.state().available != 0),
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
    let app_interval = crate::frame_limit::application_interval(limit, &state);
    let anti_paced = anti.is_some_and(|anti| anti.pace(id, mode, app_interval, other) != 0);
    if owner == RSF_PACING_ANTILAG2 && anti.is_none_or(|anti| anti.state().available == 0) {
        owner = RSF_PACING_NONE;
    }
    lock(&PACER).owner = owner;

    // DLSS-G configures its shared Reflex host at Prepare using this frame's
    // eligibility. Other providers use the last effective presentation ratio.
    let mut configured = lock(&REFLEX_CONFIGURATION);
    if reflex_available && backend != RSF_FG_BACKEND_DLSS {
        let wanted = if owner == RSF_PACING_REFLEX {
            let presents = if state.effective_mode == RSF_FG_FIXED {
                state.effective_generated_frames.saturating_add(1)
            } else {
                1
            };
            (
                host as usize,
                mode,
                crate::frame_limit::reflex_interval(limit, presents),
            )
        } else {
            (host as usize, RSF_REFLEX_OFF, 0)
        };
        if *configured != Some(wanted) {
            *configured = (call!(host_reflex, host, wanted.1, wanted.2) == OK).then_some(wanted);
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

pub(crate) fn pacing_status(status: &mut Status) {
    let owner = lock(&PACER).owner;
    let update = match owner {
        RSF_PACING_NONE => Some((0, RSF_REFLEX_OFF, 0)),
        RSF_PACING_REFLEX => host_status(call!(host))
            .map(|s| (s.low_latency_available, s.reflex_mode, s.sleep_calls)),
        RSF_PACING_XELL => provider_status().map(|s| {
            (
                s.state.low_latency_available,
                s.state.effective_reflex,
                s.state.latency_paced_frames,
            )
        }),
        RSF_PACING_ANTILAG2 => antilag()
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

fn pace_frame(source: Source, prospective: Option<Reservation>) {
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
                    if wait_completion(1, 0) == 0 {
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
                frame.pacing = Attempt::Done;
            }
            call!(abort, source.id);
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
    } else if !threaded && !call!(host).is_null() {
        join_previous_present(claimed.previous)
    } else {
        0
    };

    let started = now_qpc();
    let options = requested();
    let generation = claimed.provider_generation;
    let selection = (generation == call!(generation))
        .then(|| select_pacer(source.id, options.reflex_mode, options.frame_limit_us));
    let actual_pacer = selection.as_ref().is_some_and(|s| {
        matches!(s.owner, RSF_PACING_REFLEX | RSF_PACING_XELL)
            || (s.owner == RSF_PACING_ANTILAG2 && s.anti_paced)
    });
    // No ledger or provider-settings lock spans this call. The presentation
    // bridge owns a read lease for the duration of the vendor's sleep.
    let result = selection
        .as_ref()
        .filter(|s| !s.denied)
        .map_or(NOT_READY, |s| call!(pace, source.id, s.owner, generation));
    let finished = now_qpc();
    let mut withdrawn = false;
    if let Some(frame) = lock(&LEDGER).exact(source) {
        frame.pacing = Attempt::Done;
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
        call!(abort, source.id);
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

fn withdraw_reserved(session: u64) {
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
            if !frame.attempted(SUBMIT_START) && !frame.submitted && !frame.pacing_in_flight() {
                *out = frame.id;
            }
        }
    }
    for id in withdrawn.into_iter().filter(|id| *id != 0) {
        call!(abort, id);
    }
}

fn reserve(source: Source) {
    let Some(reservation) = source
        .reservation()
        .filter(|r| r.state == RSF_NATIVE_CPU_RESERVATION_OPEN)
    else {
        return;
    };
    let generation = call!(generation);
    {
        let mut ledger = lock(&LEDGER);
        let frame = ledger.slot(source.id);
        if frame.source() == source
            || frame.pacing_in_flight()
            || (frame.id != 0
                && !frame.ended
                && !(frame.failed && !frame.marked(SUBMIT_START) && !frame.submitted))
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
    let acquired = frame_acquire(source);
    let cancelled = lock(&LEDGER).exact(source).is_some_and(|frame| {
        frame.failed |= acquired != OK;
        frame.failed
    });
    if cancelled {
        call!(abort, source.id);
    } else {
        pace_frame(source, Some(reservation));
    }
}

fn begin(event: &Event, source: Source) {
    let joined_at = now_qpc();
    let generation = call!(generation);
    let reserved = lock(&LEDGER)
        .exact(source)
        .is_some_and(|frame| frame.prospective);
    let result = if reserved { OK } else { frame_acquire(source) };
    if let Some(latency) = host_status(call!(host)) {
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
            call!(abort, source.id);
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

fn end(event: &Event, source: Source) {
    let required = if rendered_source() {
        (1 << RSF_GAME_CPU_FRAME_BEGIN) | (1 << RSF_GAME_CPU_SIMULATION_END)
    } else {
        THROUGH_SIM_END
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
        call!(abort, source.id);
        complete(source.id);
    }
    audit(source);
    report_latency_window();
}

fn route_receipt(receipt: &rsf_game_cpu_receipt_event) {
    let event = &receipt.header;
    if event.stage != RSF_GAME_CPU_INPUT_EVENT
        || event.source_frame_id != 0
        || (event.input_kind == 0 && rsf_native_cpu_is_ping_message(event.message_id) == 0)
    {
        return;
    }
    let generation = call!(generation);
    let (selected, binding) = {
        let mut ledger = lock(&LEDGER);
        let binding = ledger.binding;
        let thread = if binding.key == 0 {
            0
        } else {
            owned_window_thread(binding.key)
        };
        let target = binding.session == event.session_id
            && thread != 0
            && thread == binding.thread
            && if receipt.window_key != 0 {
                receipt.window_key == binding.key
            } else {
                win32::thread_id() == binding.thread
            };
        let mut candidates = [Owner::default(); CAPACITY];
        if target {
            for (candidate, frame) in candidates.iter_mut().zip(&ledger.frames) {
                if frame.id == 0
                    || frame.session != event.session_id
                    || frame.pacing != Attempt::Done
                    || frame.failed
                    || frame.marker_failed
                    || frame.ended
                    || frame.attempted(SUBMIT_START)
                    || frame.submitted
                {
                    continue;
                }
                let Some(facts) = frame.source().facts().filter(|f| {
                    f.failed == 0
                        && f.ended == 0
                        && f.stage_mask == THROUGH_SIM_BEGIN
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
                    eligible: true,
                };
            }
        }
        let selected = input_receipts::owner(
            &candidates,
            event.session_id,
            generation,
            event.timestamp_qpc,
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

unsafe fn fg_cpu(event: *const Event) {
    if event.is_null() || unsafe { event.cast::<u32>().read() } < size_of::<Event>() as u32 {
        return;
    }
    let event_ref = unsafe { &*event };
    if event_ref.event_version == RSF_GAME_CPU_EVENT_VERSION_RECEIPT {
        if event_ref.struct_size >= size_of::<rsf_game_cpu_receipt_event>() as u32 {
            route_receipt(unsafe { &*event.cast() });
        }
        return;
    }
    let stage = event_ref.stage;
    if !observed_boundary(event_ref) || call!(has_owner) == 0 {
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
        withdraw_reserved(source.session);
    }
    if (RSF_GAME_CPU_SOURCE_RESERVE..=RSF_GAME_CPU_SOURCE_ABORT).contains(&stage) {
        if !rendered && stage == RSF_GAME_CPU_SOURCE_RESERVE {
            reserve(source);
        }
        return;
    }
    match stage {
        RSF_GAME_CPU_FRAME_BEGIN => begin(event_ref, source),
        RSF_GAME_CPU_FRAME_END => end(event_ref, source),
        _ if !rendered => dispatch_boundary(event_ref, source),
        _ => {}
    }
}

fn dispatch_boundary(event: &Event, source: Source) {
    let generation = call!(generation);
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
    // A reservation already ran the pacer before its actual poll.
    if pacing_boundary(event) && (bound.is_none() || event.stage == RSF_GAME_CPU_PACING) {
        pace_frame(source, None);
        return;
    }
    match event.stage {
        RSF_GAME_CPU_SIMULATION_BEGIN if bound.is_none_or(|r| r.poll_begin_qpc != 0) => {
            simulation_marker(source, SIM_START, "actual simulation begin");
        }
        RSF_GAME_CPU_SIMULATION_END => {
            simulation_marker(source, SIM_END, "simulation end");
        }
        RSF_GAME_CPU_INPUT_EVENT
            if !frame.marker_failed
                && frame.provider_generation == generation
                && (!frame.prospective
                    || event.flags
                        & (RSF_GAME_CPU_INPUT_UNFRAMED | RSF_GAME_CPU_WINDOW_MESSAGE)
                        == 0) =>
        {
            trace(
                "input received",
                source.id,
                call!(input, source.id, event.input_kind, event.message_id),
            );
        }
        _ => {}
    }
}

fn start_render_marker(id: u64) {
    let generation = call!(generation);
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
        let Some(frame) = claim_marker(frame, SUBMIT_START) else {
            return;
        };
        frame
    };
    let source = frame.source();
    let finish = |accepted, failed| {
        settle_marker(source, SUBMIT_START, accepted, failed);
    };
    if rendered_source() {
        finish(true, false);
        return;
    }
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
        let missing = f.stage_mask & THROUGH_SIM_BEGIN != THROUGH_SIM_BEGIN;
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
    if f.stage_mask & THROUGH_SIM_END != THROUGH_SIM_END
        || !frame.marked(SIM_START)
        || !frame.marked(SIM_END)
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
    let (result, marked) = deliver_marker(frame, SUBMIT_START);
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

fn end_render_marker(id: u64, stage: &str) {
    let frame = {
        let mut ledger = lock(&LEDGER);
        let Some(frame) = ledger.frame(id) else {
            return;
        };
        if !frame.marked(SUBMIT_START) {
            return;
        }
        let Some(frame) = claim_marker(frame, SUBMIT_END) else {
            return;
        };
        frame
    };
    if !rendered_source()
        && !frame.source().facts().is_some_and(|f| {
            let required = marker_prerequisites(f.flags, SUBMIT_END);
            f.failed == 0 && f.marker_mask & required == required
        })
    {
        settle_marker(frame.source(), SUBMIT_END, false, false);
        return;
    }
    let (result, _) = deliver_marker(frame, SUBMIT_END);
    trace(stage, id, result);
}

pub(crate) fn latency_present(id: u64, chain: Handle, begin: bool) {
    if id == 0 {
        return;
    }
    if begin {
        let threaded = THREADED.load(Ordering::Relaxed);
        if !threaded {
            end_render_marker(id, "render submission end before present");
        }
        let frame = {
            let mut ledger = lock(&LEDGER);
            let Some(frame) = ledger.frame(id) else {
                return;
            };
            if !frame.marked(SUBMIT_START) {
                let frame = *frame;
                drop(ledger);
                if !frame.submitted
                    && !frame.attempted(SUBMIT_START)
                    && !frame.failed
                    && !frame.marker_failed
                    && !rendered_source()
                {
                    failure(
                        Failure::PresentWithoutSubmission,
                        frame.source(),
                        NOT_READY,
                        0,
                        frame.provider_generation,
                        frame.source().facts().as_ref(),
                        None,
                    );
                }
                return;
            }
            let Some(frame) = claim_marker(frame, PRESENT_START) else {
                return;
            };
            frame
        };
        let rendered = rendered_source();
        let facts = frame.source().facts();
        let keyed = facts.is_some_and(|f| f.flags & RSF_GAME_CPU_INPUT_KEYED != 0);
        if ((keyed || !threaded) && !frame.marked(SUBMIT_END))
            || (!rendered
                && !facts.is_some_and(|f| {
                    let required = marker_prerequisites(f.flags, PRESENT_START);
                    f.failed == 0 && f.marker_mask & required == required
                }))
        {
            settle_marker(frame.source(), PRESENT_START, false, false);
            return;
        }
        if !rendered && let Some(anti) = antilag() {
            anti.end_render(id, chain, u32::from(fsr(call!(backend))));
        }
        let (result, _) = deliver_marker(frame, PRESENT_START);
        trace("present begin", id, result);
    } else {
        let frame = lock(&LEDGER).frame(id).and_then(|frame| {
            if !frame.marked(PRESENT_START) {
                return None;
            }
            claim_marker(frame, PRESENT_END)
        });
        if let Some(frame) = frame {
            let (result, _) = deliver_marker(frame, PRESENT_END);
            trace("present end", id, result);
        }
        complete(id);
    }
}

fn has_pass_size(pass: *const Pass) -> bool {
    !pass.is_null() && unsafe { pass.cast::<u32>().read() } >= size_of::<Pass>() as u32
}

unsafe fn fg_frame(pass: *const Pass, begin: u32) {
    if !has_pass_size(pass) {
        return;
    }
    let pass = unsafe { &*pass };
    let source = Source::from_pass(pass);
    if pass.role != RSF_GAME_RENDER_FRAME || source.id == 0 || call!(has_owner) == 0 {
        return;
    }
    {
        let mut ledger = lock(&LEDGER);
        if ledger.exact(source).is_none() {
            return;
        }
        if begin != 0 && rendered_source() {
            ledger.rendered_begin(pass, call!(generation), now_qpc(), win32::qpc_frequency());
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
    end_render_marker(source.id, "full RHI frame end");
    if progress(source.id).0 {
        call!(abort, source.id);
    }
    complete(source.id);
    if RHI_SOURCE.get() == source {
        RHI_SOURCE.set(Source::default());
    }
    audit(source);
}

fn watch(pass: &Pass, begin: bool) {
    if !begin {
        hitch::close(Source::from_pass(pass), pass.submission_id);
        return;
    }
    if requested().debug == 0 {
        return;
    }
    let now = win32::tick_millis();
    let (screen, since) = SHOWN.get();
    if screen != pass.screen {
        SHOWN.set((pass.screen, now));
        return;
    }
    let mut record: rsf_frame_record = unsafe { zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.screen = screen;
    if now.saturating_sub(since) >= 3000 && call!(allows_fg, &record) != 0 {
        hitch::open(Source::from_pass(pass), pass.submission_id, now_qpc());
    }
}

unsafe fn fg_submission(pass: *const Pass, begin: u32) {
    if !has_pass_size(pass) {
        return;
    }
    let pass = unsafe { &*pass };
    let source = Source::from_pass(pass);
    let required = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
    if source.id == 0 || pass.flags & required != required || call!(has_owner) == 0 {
        return;
    }
    watch(pass, begin != 0);
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
            if frame.ended && frame.unsubmitted_before(source) {
                frame.failed = true;
                *id = frame.id;
            }
        }
        start
    };
    for id in dropped.into_iter().filter(|id| *id != 0) {
        call!(abort, id);
    }
    if start {
        start_render_marker(source.id);
    }
}

extern "C" fn rsf_core_native_fg_reflex_ordering_set(threaded: u32) {
    THREADED.store(threaded != 0, Ordering::Relaxed);
}

extern "C" fn rsf_core_native_latency_graphics(api: u32) {
    if matches!(api, 11 | 12) {
        GRAPHICS_API.store(api, Ordering::Relaxed);
    }
}

fn latency_render(id: u64, begin: u32) {
    if begin != 0 {
        start_render_marker(id);
    } else {
        end_render_marker(id, "render submission end");
    }
}

fn observe_latency_present(id: u64, chain: Handle, begin: u32) {
    latency_present(id, chain, begin != 0);
    if begin == 0 && id != 0 {
        let host = call!(host);
        if !host.is_null() {
            call!(host_abort, host, id);
        }
    }
}

fn empty_routes() -> Routes {
    Routes {
        struct_size: size_of::<Routes>() as u32,
        version: 1,
        ..Routes::default()
    }
}

unsafe fn latency_window_bind(binding: *const Window) -> i32 {
    if !unsafe { has_capacity(binding) } {
        return INVALID;
    }
    let binding = unsafe { &*binding };
    if binding.session_id == 0 || binding.window_key == 0 {
        return INVALID;
    }
    if binding.version != 1 {
        return ABI_MISMATCH;
    }
    bind_window(binding.session_id, binding.window_key)
}

pub(crate) fn bind_window(session_id: u64, window_key: u64) -> i32 {
    if session_id == 0 || window_key == 0 {
        return INVALID;
    }
    let thread = owned_window_thread(window_key);
    if thread == 0 {
        return INVALID;
    }
    let mut ledger = lock(&LEDGER);
    let mut epoch = ledger.binding.epoch;
    if (
        ledger.binding.session,
        ledger.binding.key,
        ledger.binding.thread,
    ) != (session_id, window_key, thread)
    {
        ledger.routes = empty_routes();
        epoch = epoch.wrapping_add(1);
    }
    ledger.binding = Binding {
        key: window_key,
        session: session_id,
        thread,
        epoch,
    };
    OK
}

pub(crate) unsafe extern "C" fn rsf_core_native_input_routes_get(out: *mut Routes) -> i32 {
    if !unsafe { has_capacity(out) } || unsafe { (*out).version } != 1 {
        return 0;
    }
    unsafe { out.write(lock(&LEDGER).routes) };
    1
}

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
    out.failed = u32::from(frame.failed || frame.marker_failed);
    out.completed = u32::from(COMPLETED.load(Ordering::Acquire) >= frame.id);
    1
}

fn registered_ping(message: u32) -> bool {
    (0xc000..=0xffff).contains(&message)
}

unsafe fn latency_status_get(out: *mut LatencyStatus) -> i32 {
    if !unsafe { has_capacity(out) } || unsafe { (*out).version } != 1 {
        return 0;
    }
    let mut out_value: LatencyStatus = unsafe { zeroed() };
    out_value.struct_size = size_of::<LatencyStatus>() as u32;
    out_value.version = 1;
    if !rendered_source() {
        let mut status: Status = unsafe { zeroed() };
        pacing_status(&mut status);
        out_value.owner = status.pacing_owner;
        out_value.available = status.low_latency_available;
        out_value.effective_mode = status.effective_reflex;
        out_value.paced_frames = status.latency_paced_frames;
        if let Some(sdk) = host_status(call!(host)) {
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

fn latency_ping(window: Handle) -> i32 {
    i32::from(
        !window.is_null()
            && host_status(call!(host)).is_some_and(|sdk| {
                registered_ping(sdk.pcl_message_id)
                    && win32::post_ping(window, sdk.pcl_message_id) != 0
            }),
    )
}

fn latency_retire(session: u64, through: u64) {
    // The caller reports older sources with no submitted GPU work.
    if through == 0 || !owns_session(session) {
        return;
    }
    let host = call!(host);
    if !host.is_null() {
        call!(host_retire, host, through);
    }
    for frame in &mut lock(&LEDGER).frames {
        if frame.session == session
            && frame.id != 0
            && frame.id <= through
            && !frame.marked(SUBMIT_START)
        {
            frame.failed = true;
        }
    }
    complete(through);
}

fn latency_end_session(session: u64) {
    // All producers and leased graphics work have drained. The CPU dispatcher
    // owns admission and can refuse shutdown of a different session.
    if rsf_native_cpu_release_session(session) == 0 {
        return;
    }
    hitch::end_session(session);
    let host = call!(host);
    if !host.is_null() {
        call!(host_retire, host, u64::MAX);
        call!(host_reset, host);
        call!(host_reflex, host, RSF_REFLEX_OFF, 0);
    }
    {
        let mut ledger = lock(&LEDGER);
        let epoch = ledger.binding.epoch.wrapping_add(1);
        *ledger = Ledger::new();
        ledger.binding.epoch = epoch;
    }
    if let Some(anti) = antilag() {
        anti.reset_sources();
    }
    lock(&PACER).owner = RSF_PACING_NONE;
    *lock(&REFLEX_CONFIGURATION) = None;
    COMPLETED.store(0, Ordering::Release);
    JOIN_SKIP.store(0, Ordering::Relaxed);
    DERIVED.store(0, Ordering::Relaxed);
    rsf_native_latency_release_claim(D3D_CLAIM);
}
