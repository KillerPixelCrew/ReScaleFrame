// SPDX-License-Identifier: MIT
//! Native CPU facts: frame records, pending window and unframed input, input
//! receipts, vendor marker bookkeeping and the source reservation ledger share
//! one lock. Sinks and recursive dispatch run only after it is released.
use crate::cpu_abi::*;
use crate::cpu_reservations::Book;
use crate::input_receipts::Window as ReceiptWindow;
use rsf_game_sys::*;
use std::ffi::c_void;
use std::mem::{offset_of, zeroed};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

type Event = rsf_game_cpu_event;
type Sink = rsf_game_cpu_event_fn;

const FRAMES: usize = 128;
const PENDING: usize = 16;
const EVENT_SIZE: u32 = size_of::<Event>() as u32;
/// RSF_GAME_CPU_SOURCE_EVENT_SIZE_1: the first source packet ends before poll_begin_qpc.
const SOURCE_EVENT_SIZE_1: u32 = EVENT_SIZE + 24;
const _: () =
    assert!(SOURCE_EVENT_SIZE_1 as usize == offset_of!(rsf_game_cpu_source_event, poll_begin_qpc));
const KEYED: u32 = RSF_GAME_CPU_INPUT_KEYED;
const SUBSTITUTE: u32 = RSF_GAME_CPU_INPUT_SUBSTITUTE;
const WINDOW_MESSAGE: u32 = RSF_GAME_CPU_WINDOW_MESSAGE;
const RENDER_SKIPPED: u32 = RSF_GAME_CPU_RENDER_SKIPPED;
const UNFRAMED: u32 = RSF_GAME_CPU_INPUT_UNFRAMED;
const DERIVED: u32 = RSF_GAME_CPU_DERIVED_BOUNDARY;
const KNOWN_FLAGS: u32 = KEYED | SUBSTITUTE | WINDOW_MESSAGE | RENDER_SKIPPED | UNFRAMED | DERIVED;
const FRAME_BEGIN: u32 = RSF_GAME_CPU_FRAME_BEGIN;
const INPUT_SAMPLE: u32 = RSF_GAME_CPU_INPUT_SAMPLE;
const SIMULATION_BEGIN: u32 = RSF_GAME_CPU_SIMULATION_BEGIN;
const SIMULATION_END: u32 = RSF_GAME_CPU_SIMULATION_END;
const FRAME_END: u32 = RSF_GAME_CPU_FRAME_END;
const INPUT_EVENT: u32 = RSF_GAME_CPU_INPUT_EVENT;
const PACING: u32 = RSF_GAME_CPU_PACING;
pub(crate) const fn stage_bit(stage: u32) -> u32 {
    1 << stage
}

const BEGUN: u32 = stage_bit(FRAME_BEGIN);
const BEGUN_INPUT: u32 = BEGUN | stage_bit(INPUT_SAMPLE);
const BEGUN_SIMULATION: u32 = BEGUN | stage_bit(SIMULATION_BEGIN);
const BEGUN_SIM_END: u32 = BEGUN | stage_bit(SIMULATION_END);
const BEGUN_SIM_INTERVAL: u32 = BEGUN_SIMULATION | stage_bit(SIMULATION_END);
pub(crate) const THROUGH_SIM_BEGIN: u32 = BEGUN_INPUT | stage_bit(SIMULATION_BEGIN);
pub(crate) const THROUGH_SIM_END: u32 = THROUGH_SIM_BEGIN | stage_bit(SIMULATION_END);

pub(crate) fn observed_boundary(event: &Event) -> bool {
    event.flags & (SUBSTITUTE | DERIVED) == 0
        || !matches!(
            event.stage,
            INPUT_SAMPLE | SIMULATION_BEGIN | SIMULATION_END
        )
}

pub(crate) fn input_keyed(event: &Event) -> bool {
    event.event_version == RSF_GAME_CPU_EVENT_VERSION && event.flags & KEYED != 0
}

pub(crate) fn pacing_boundary(event: &Event) -> bool {
    event.stage
        == if input_keyed(event) {
            INPUT_SAMPLE
        } else {
            PACING
        }
}

/// Legacy threaded hosts report overlapping simulation/render and EndFrame after
/// Present. Preserve those dependencies until they migrate; input-keyed facts use
/// canonical order.
const LEGACY_MARKER_REQUIRED: [u32; 6] = [0, 1, 1, 4, 4, 16];

const EMPTY_EVENT: Event = unsafe { zeroed() };
const EMPTY_RECORD: rsf_native_cpu_frame = unsafe { zeroed() };

/// Provenance belongs to the existing frame slot, never to another source ledger.
#[derive(Clone, Copy)]
struct CpuFrame {
    record: rsf_native_cpu_frame,
    derived_stages: u32,
}

const EMPTY_FRAME: CpuFrame = CpuFrame {
    record: EMPTY_RECORD,
    derived_stages: 0,
};

struct State {
    frames: [CpuFrame; FRAMES],
    current_session: u64,
    last_begin: u64,
    session_frequency: u64,
    consumer: Sink,
    consumer_user: *mut c_void,
    totals: rsf_native_cpu_counters,
    ping_message: u32,
    /// Receipt replay window, serialized by the state lock.
    receipt_window: ReceiptWindow,
    receipts: rsf_native_input_receipts,
    pending: [Event; PENDING],
    next_message: u32,
    ledger: Book,
}

// SAFETY: consumer_user is opaque caller state that is only handed back to the
// consumer. The public contract requires it to outlive callbacks, not a thread.
unsafe impl Send for State {}

impl State {
    const fn new() -> Self {
        Self {
            frames: [EMPTY_FRAME; FRAMES],
            current_session: 0,
            last_begin: 0,
            session_frequency: 0,
            consumer: None,
            consumer_user: ptr::null_mut(),
            totals: rsf_native_cpu_counters {
                struct_size: size_of::<rsf_native_cpu_counters>() as u32,
                ..unsafe { zeroed() }
            },
            ping_message: 0,
            receipt_window: ReceiptWindow::new(),
            receipts: rsf_native_input_receipts {
                struct_size: size_of::<rsf_native_input_receipts>() as u32,
                version: 1,
                ..unsafe { zeroed() }
            },
            pending: [EMPTY_EVENT; PENDING],
            next_message: 0,
            ledger: Book::new(),
        }
    }

    fn slot(&mut self, source: u64) -> &mut CpuFrame {
        &mut self.frames[(source % FRAMES as u64) as usize]
    }

    /// A supplied sink takes precedence over the configured consumer.
    fn route(&self, sink: Sink, user: *mut c_void) -> (Sink, *mut c_void) {
        if sink.is_some() {
            (sink, user)
        } else {
            (self.consumer, self.consumer_user)
        }
    }
}

fn deliver(route: (Sink, *mut c_void), event: *const Event) {
    if let (Some(sink), user) = route {
        unsafe { sink(user, event) };
    }
}

fn qpc_now() -> u64 {
    let mut now = 0i64;
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
    now as u64
}

unsafe fn has_capacity<T>(value: *const T) -> bool {
    !value.is_null() && (unsafe { value.cast::<u32>().read() } as usize) >= size_of::<T>()
}

struct Dispatcher {
    rendered: AtomicBool,
    state: Mutex<State>,
}

static CPU: Dispatcher = Dispatcher {
    rendered: AtomicBool::new(false),
    state: Mutex::new(State::new()),
};

impl Dispatcher {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn rendered(&self) -> bool {
        self.rendered.load(Ordering::SeqCst)
    }

    fn read(&self, session: u64, source: u64) -> Option<rsf_native_cpu_frame> {
        if source == 0 {
            return None;
        }
        let mut state = self.lock();
        let record = state.slot(source).record;
        (record.session_id == session && record.source_frame_id == source).then_some(record)
    }

    fn reservation(&self, session: u64, source: u64) -> Option<rsf_native_cpu_reservation> {
        let state = self.lock();
        if session == state.current_session {
            state.ledger.read(session, source)
        } else {
            None
        }
    }

    /// Version 2 source facts. Returns whether the ledger accepted them.
    fn source_event(&self, incoming: *const Event, header: &Event) -> bool {
        let mut guard = self.lock();
        let state = &mut *guard;
        let rendered = self.rendered();
        let size = header.struct_size;
        if header.event_version != RSF_GAME_CPU_EVENT_VERSION_SOURCE
            || size < SOURCE_EVENT_SIZE_1
            || (size > SOURCE_EVENT_SIZE_1
                && (size as usize) < size_of::<rsf_game_cpu_source_event>())
        {
            let mut unused = EMPTY_RECORD;
            return state.ledger.source_event(
                None,
                state.current_session,
                state.session_frequency,
                state.last_begin,
                false,
                &mut unused,
                rendered,
            );
        }
        // A short version 2 packet ends before poll_begin_qpc, which stays zero.
        let mut event: rsf_game_cpu_source_event = unsafe { zeroed() };
        let copied = (size as usize).min(size_of::<rsf_game_cpu_source_event>());
        unsafe {
            ptr::copy_nonoverlapping(
                incoming.cast::<u8>(),
                ptr::from_mut(&mut event).cast::<u8>(),
                copied,
            )
        };
        let predecessor = event.predecessor_source_id;
        let owner = state.slot(predecessor).record;
        let known_predecessor = predecessor == 0
            || (predecessor <= state.last_begin
                && owner.source_frame_id == predecessor
                && owner.session_id == event.header.session_id);
        let (session, frequency, last_begin) = (
            state.current_session,
            state.session_frequency,
            state.last_begin,
        );
        let State { frames, ledger, .. } = state;
        let frame = &mut frames[(event.header.source_frame_id % FRAMES as u64) as usize].record;
        ledger.source_event(
            Some(&event),
            session,
            frequency,
            last_begin,
            known_predecessor,
            frame,
            rendered,
        )
    }

    fn receipt(&self, incoming: *const Event, sink: Sink, user: *mut c_void) -> i32 {
        let mut receipt = unsafe { incoming.cast::<rsf_game_cpu_receipt_event>().read() };
        receipt.header.struct_size = size_of::<rsf_game_cpu_receipt_event>() as u32;
        let route = {
            let mut guard = self.lock();
            let state = &mut *guard;
            let e = &receipt.header;
            let now = qpc_now();
            if e.stage != INPUT_EVENT
                || e.source_frame_id != 0
                || e.session_id == 0
                || e.session_id != state.current_session
                || e.qpc_frequency != state.session_frequency
                || e.timestamp_qpc == 0
                || e.timestamp_qpc > now
                || receipt.receipt_id == 0
                || e.render_expected != 0
                || e.flags & UNFRAMED == 0
                || e.flags & !(UNFRAMED | WINDOW_MESSAGE) != 0
                || e.input_kind & !7 != 0
            {
                state.receipts.rejected = state.receipts.rejected.wrapping_add(1);
                return 0;
            }
            if !state.receipt_window.accept(receipt.receipt_id) {
                state.receipts.duplicates = state.receipts.duplicates.wrapping_add(1);
                return 0;
            }
            let totals = &mut state.receipts;
            totals.session_id = state.current_session;
            totals.accepted = totals.accepted.wrapping_add(1);
            for (kind, count) in totals.input_kinds.iter_mut().enumerate() {
                if e.input_kind & (1 << kind) != 0 {
                    *count = count.wrapping_add(1);
                }
            }
            if e.flags & WINDOW_MESSAGE != 0 {
                totals.window_messages = totals.window_messages.wrapping_add(1);
            }
            if state.ping_message != 0 && e.message_id == state.ping_message {
                totals.ping_candidates = totals.ping_candidates.wrapping_add(1);
            }
            state.route(sink, user)
        };
        deliver(route, ptr::from_ref(&receipt).cast());
        1
    }

    fn source(&self, incoming: *const Event, header: &Event, sink: Sink, user: *mut c_void) -> i32 {
        // Source facts are stored before the synchronous runtime callback. Rendered-only
        // hosts retain their observation-only behavior; input hosts may pace at RESERVE.
        let accepted = self.source_event(incoming, header);
        let mut route = (None, ptr::null_mut());
        if accepted && !self.rendered() {
            route = self.lock().route(sink, user);
        }
        deliver(route, incoming);
        if accepted && !self.rendered() && header.stage == RSF_GAME_CPU_SOURCE_BIND {
            let mut derived = EMPTY_EVENT;
            {
                let mut state = self.lock();
                let reservation = state.ledger.get(header.session_id, header.source_frame_id);
                let frame = state.slot(header.source_frame_id).record;
                if let Some(reservation) = reservation
                    && reservation.state == RSF_NATIVE_CPU_RESERVATION_BOUND
                    && reservation.poll_begin_qpc != 0
                    && frame.session_id == header.session_id
                    && frame.source_frame_id == header.source_frame_id
                    && frame.failed == 0
                    && frame.ended == 0
                    && frame.stage_mask == BEGUN_INPUT
                    && frame.flags & SUBSTITUTE == 0
                {
                    derived = Event {
                        struct_size: EVENT_SIZE,
                        stage: SIMULATION_BEGIN,
                        session_id: frame.session_id,
                        source_frame_id: frame.source_frame_id,
                        timestamp_qpc: reservation.bind_qpc,
                        qpc_frequency: frame.qpc_frequency,
                        event_version: RSF_GAME_CPU_EVENT_VERSION,
                        flags: KEYED | DERIVED,
                        ..EMPTY_EVENT
                    };
                }
            }
            // The source sink may reenter or withdraw the reservation. Normal dispatch
            // validates it again; neither token acquisition nor pacing runs here.
            if derived.source_frame_id != 0 {
                self.dispatch(&derived, sink, user);
            }
        }
        accepted as i32
    }

    fn dispatch(&self, incoming: *const Event, sink: Sink, user: *mut c_void) -> i32 {
        if !unsafe { has_capacity(incoming) } {
            return 0;
        }
        let header = unsafe { incoming.read() };
        if header.event_version == RSF_GAME_CPU_EVENT_VERSION_RECEIPT {
            if (header.struct_size as usize) < size_of::<rsf_game_cpu_receipt_event>() {
                return 0;
            }
            return self.receipt(incoming, sink, user);
        }
        if (RSF_GAME_CPU_SOURCE_RESERVE..=RSF_GAME_CPU_SOURCE_ABORT).contains(&header.stage) {
            return self.source(incoming, &header, sink, user);
        }
        if header.event_version > RSF_GAME_CPU_EVENT_VERSION || header.flags & !KNOWN_FLAGS != 0 {
            return 0;
        }
        let rendered = self.rendered();
        if rendered && (header.stage == INPUT_SAMPLE || header.stage == PACING) {
            return 0;
        }
        let mut copy = Event {
            struct_size: EVENT_SIZE,
            ..header
        };
        self.fact(&mut copy, rendered, sink, user)
    }

    /// Version 0 and 1 CPU facts. `event` is the private copy delivered to the sink.
    fn fact(&self, event: &mut Event, rendered: bool, sink: Sink, user: *mut c_void) -> i32 {
        let version_one = event.event_version == RSF_GAME_CPU_EVENT_VERSION;
        let window_message = version_one && event.flags & WINDOW_MESSAGE != 0;
        let unframed = version_one && event.flags & UNFRAMED != 0;
        if (window_message || unframed) && event.stage == INPUT_EVENT && event.source_frame_id == 0
        {
            if event.session_id == 0 || event.timestamp_qpc == 0 || event.qpc_frequency == 0 {
                return 0;
            }
            if event.input_kind & !7 != 0 {
                return 0;
            }
            if event.input_kind == 0 && !(0xc000..=0xffff).contains(&event.message_id) {
                return 1;
            }
            if event.input_kind != 0 && !unframed {
                return 0;
            }
            let mut state = self.lock();
            if state.current_session != 0 && state.current_session != event.session_id {
                return 0;
            }
            let index = state.next_message as usize % PENDING;
            state.next_message = state.next_message.wrapping_add(1);
            state.pending[index] = *event;
            return 1;
        }
        let facts = version_one && event.flags & KEYED != 0;
        if facts && event.stage == FRAME_BEGIN {
            let mut previous = EMPTY_RECORD;
            {
                let mut state = self.lock();
                if state.current_session == event.session_id
                    && state.last_begin != 0
                    && state.last_begin < event.source_frame_id
                {
                    let last = state.last_begin;
                    previous = state.slot(last).record;
                }
            }
            if (previous.stage_mask == THROUGH_SIM_END
                || (rendered && previous.stage_mask & BEGUN_SIM_END == BEGUN_SIM_END))
                && previous.ended == 0
            {
                let end = Event {
                    stage: FRAME_END,
                    source_frame_id: previous.source_frame_id,
                    flags: event.flags | DERIVED,
                    render_expected: 0,
                    ..*event
                };
                if self.dispatch(&end, sink, user) == 0 {
                    return 0;
                }
            }
        }
        // A renderer can run without an engine input callback (loading/first frame).
        // Normalize that absence centrally, label it, and skip a late pacing sleep.
        // Adapters report only the simulation end they actually observed.
        if !rendered && facts && event.stage == SIMULATION_END {
            let Some(current) = self.read(event.session_id, event.source_frame_id) else {
                return 0;
            };
            let found = self.reservation(event.session_id, event.source_frame_id);
            let refused_binding = found
                .is_some_and(|reservation| reservation.state != RSF_NATIVE_CPU_RESERVATION_BOUND);
            let mut substitute = *event;
            substitute.flags |= SUBSTITUTE;
            if !refused_binding
                && current.stage_mask & stage_bit(FRAME_BEGIN) != 0
                && current.stage_mask & stage_bit(INPUT_SAMPLE) == 0
            {
                substitute.stage = INPUT_SAMPLE;
                if self.dispatch(&substitute, sink, user) == 0 {
                    return 0;
                }
            }
            if !refused_binding
                && current.stage_mask & stage_bit(FRAME_BEGIN) != 0
                && current.stage_mask & stage_bit(SIMULATION_BEGIN) == 0
            {
                substitute.stage = SIMULATION_BEGIN;
                if let Some(reservation) = found
                    && reservation.state == RSF_NATIVE_CPU_RESERVATION_BOUND
                    && reservation.poll_begin_qpc != 0
                    && current.stage_mask & stage_bit(INPUT_SAMPLE) != 0
                    && current.flags & SUBSTITUTE == 0
                {
                    substitute.timestamp_qpc = reservation.bind_qpc;
                    substitute.flags = KEYED | DERIVED;
                }
                if self.dispatch(&substitute, sink, user) == 0 {
                    return 0;
                }
            }
        }
        if event.stage > PACING
            || event.session_id == 0
            || event.source_frame_id == 0
            || event.timestamp_qpc == 0
            || event.qpc_frequency == 0
        {
            return 0;
        }
        if let Some(result) = self.correct_derived(event, sink, user) {
            return result;
        }
        let route = {
            let mut guard = self.lock();
            match record(
                &mut guard,
                event,
                facts,
                rendered,
                window_message,
                unframed,
                sink,
                user,
            ) {
                Ok(route) => route,
                Err(result) => return result,
            }
        };
        deliver(route, event);
        if event.stage == FRAME_BEGIN && facts {
            self.flush_pending(event, sink, user);
        }
        1
    }

    /// An observed simulation boundary replaces the derived one it was normalized from.
    fn correct_derived(&self, event: &mut Event, sink: Sink, user: *mut c_void) -> Option<i32> {
        if (event.stage != SIMULATION_BEGIN && event.stage != SIMULATION_END)
            || event.flags & (DERIVED | SUBSTITUTE) != 0
        {
            return None;
        }
        let route = {
            let mut guard = self.lock();
            let state = &mut *guard;
            let current_session = state.current_session;
            let frame = state.slot(event.source_frame_id);
            let record = &mut frame.record;
            if record.session_id != event.session_id
                || current_session != event.session_id
                || record.source_frame_id != event.source_frame_id
                || record.failed != 0
                || record.ended != 0
                || frame.derived_stages & (1 << event.stage) == 0
            {
                return None;
            }
            let stamps = &record.timestamps_qpc;
            let preceding = if event.stage == SIMULATION_BEGIN {
                stamps[FRAME_BEGIN as usize]
            } else {
                stamps[SIMULATION_BEGIN as usize]
            };
            if event.qpc_frequency != record.qpc_frequency
                || event.timestamp_qpc < preceding
                || (event.stage == SIMULATION_BEGIN
                    && record.stage_mask & stage_bit(SIMULATION_END) != 0
                    && event.timestamp_qpc > stamps[SIMULATION_END as usize])
            {
                return Some(0);
            }
            record.timestamps_qpc[event.stage as usize] = event.timestamp_qpc;
            frame.derived_stages &= !(1 << event.stage);
            if frame.derived_stages == 0 {
                frame.record.flags &= !DERIVED;
            }
            if frame.record.flags & KEYED != 0 {
                event.flags |= KEYED;
            }
            state.route(sink, user)
        };
        deliver(route, event);
        Some(1)
    }

    /// Window and unframed observations become this frame's input only within a quarter second.
    fn flush_pending(&self, event: &Event, sink: Sink, user: *mut c_void) {
        let mut ready = [EMPTY_EVENT; PENDING];
        let mut count = 0;
        {
            let mut guard = self.lock();
            let state = &mut *guard;
            if state.current_session != event.session_id {
                return;
            }
            for pending in &mut state.pending {
                if pending.message_id == 0 && pending.input_kind == 0 {
                    continue;
                }
                if pending.session_id == event.session_id
                    && (pending.message_id == state.ping_message
                        || (pending.input_kind != 0 && pending.flags & UNFRAMED != 0))
                    && pending.qpc_frequency == event.qpc_frequency
                {
                    if pending.timestamp_qpc > event.timestamp_qpc {
                        continue;
                    }
                    if event.timestamp_qpc - pending.timestamp_qpc <= event.qpc_frequency / 4 {
                        let mut message = *pending;
                        message.source_frame_id = event.source_frame_id;
                        if message.flags & UNFRAMED == 0 {
                            message.timestamp_qpc = event.timestamp_qpc;
                        }
                        ready[count] = message;
                        count += 1;
                    } else {
                        state.totals.expired_messages =
                            state.totals.expired_messages.wrapping_add(1);
                    }
                }
                *pending = EMPTY_EVENT;
            }
        }
        for message in &ready[..count] {
            self.dispatch(message, sink, user);
        }
    }
}

/// Records one framed fact under the lock. `Ok` carries the sink to run after
/// unlocking, or none; `Err` is the immediate result.
#[allow(clippy::too_many_arguments)]
fn record(
    state: &mut State,
    event: &mut Event,
    facts: bool,
    rendered: bool,
    window_message: bool,
    unframed: bool,
    sink: Sink,
    user: *mut c_void,
) -> Result<(Sink, *mut c_void), i32> {
    let (session, source, stage) = (event.session_id, event.source_frame_id, event.stage);
    if state.current_session == 0 && stage == FRAME_BEGIN {
        state.current_session = session;
        state.session_frequency = event.qpc_frequency;
    }
    if state.current_session != session {
        return Err(0);
    }
    let last_begin = state.last_begin;
    let index = (source % FRAMES as u64) as usize;
    if stage == FRAME_BEGIN {
        let frame = &state.frames[index].record;
        if source <= last_begin || (frame.source_frame_id != 0 && frame.ended == 0) {
            return Err(0);
        }
        state.ledger.begin(event);
        state.frames[index] = CpuFrame {
            record: rsf_native_cpu_frame {
                struct_size: size_of::<rsf_native_cpu_frame>() as u32,
                session_id: session,
                source_frame_id: source,
                qpc_frequency: event.qpc_frequency,
                flags: event.flags,
                ..EMPTY_RECORD
            },
            derived_stages: 0,
        };
        state.last_begin = source;
        state.totals.frames = state.totals.frames.wrapping_add(1);
    } else {
        let frame = &mut state.frames[index].record;
        if frame.source_frame_id != source || frame.session_id != session || frame.ended != 0 {
            return Err(0);
        }
        if frame.failed != 0 {
            if stage == FRAME_END {
                frame.ended = 1;
                state.ledger.end(session, source);
            }
            return Err(0);
        }
    }
    let State {
        frames,
        totals,
        ledger,
        ping_message,
        current_session,
        ..
    } = state;
    let slot = &mut frames[index];
    let frame = &mut slot.record;
    let canonical =
        facts || (event.event_version == RSF_GAME_CPU_EVENT_VERSION && frame.flags & KEYED != 0);
    if canonical {
        event.flags |= KEYED;
    }
    if canonical
        && stage <= FRAME_END
        && stage != FRAME_BEGIN
        && frame.stage_mask & stage_bit(stage) != 0
    {
        totals.duplicates = totals.duplicates.wrapping_add(1);
        return Err(1);
    }
    let mut forward = true;
    if stage == PACING {
        if facts {
            // Version 1 pacing is selected at INPUT_SAMPLE, not at a guessed frame-open boundary.
            return Err(1);
        }
        if frame.stage_mask != BEGUN
            || frame.pacing_qpc != 0
            || frame.qpc_frequency != event.qpc_frequency
            || event.timestamp_qpc < frame.timestamps_qpc[0]
        {
            return Err(0);
        }
        frame.pacing_qpc = event.timestamp_qpc;
    } else if stage == INPUT_EVENT {
        // Pending window/unframed observations are not proof of consumption by this
        // prospective source. Do not attribute input or PCL counts to its future token.
        if !rendered && (unframed || window_message) && ledger.get(session, source).is_some() {
            return Err(1);
        }
        let frequency = event.qpc_frequency;
        let begin = frame.timestamps_qpc[0];
        if event.input_kind & !7 != 0
            || (event.input_kind == 0 && event.message_id < 0xc000 && !window_message)
            || frequency != frame.qpc_frequency
            || (!unframed && event.timestamp_qpc < begin)
            || (unframed
                && begin > event.timestamp_qpc
                && begin - event.timestamp_qpc > frequency / 4)
        {
            return Err(0);
        }
        if window_message && event.message_id != *ping_message && event.input_kind == 0 {
            return Err(1);
        }
        if unframed && event.input_kind != 0 {
            totals.unframed_inputs = totals.unframed_inputs.wrapping_add(1);
        }
        if facts {
            let previous = event.input_kind;
            event.input_kind &= !frame.input_mask;
            if previous != event.input_kind {
                totals.duplicates = totals.duplicates.wrapping_add(1);
            }
        }
        if event.message_id != 0 && event.message_id == *ping_message {
            if frame.pings != 0 {
                event.message_id = 0;
                totals.duplicates = totals.duplicates.wrapping_add(1);
            } else {
                frame.pings = 1;
                totals.pings = totals.pings.wrapping_add(1);
            }
        }
        if event.input_kind == 0 && event.message_id == 0 {
            return Err(1);
        }
        for kind in 0..3 {
            if event.input_kind & (1 << kind) != 0 {
                frame.input_mask |= 1 << kind;
                frame.input_events[kind] = frame.input_events[kind].wrapping_add(1);
                totals.input_events[kind] = totals.input_events[kind].wrapping_add(1);
                frame.input_qpc[kind] = event.timestamp_qpc;
            }
        }
    } else {
        let mask = frame.stage_mask;
        let expected = (1u32 << stage) - 1;
        // Idle/loading frames may close before input or simulation; they remain explicit partial
        // CPU records. They cannot be promoted to a complete render or latency frame.
        let partial_end = stage == FRAME_END
            && (mask == BEGUN
                || mask == BEGUN_INPUT
                || (mask == THROUGH_SIM_BEGIN && event.flags & RENDER_SKIPPED != 0));
        let reservation = ledger.get(session, source);
        let refused_binding = !rendered
            && canonical
            && reservation.is_some_and(|entry| entry.state != RSF_NATIVE_CPU_RESERVATION_BOUND);
        // Keep actual simulation boundaries when consumption was refused. Missing
        // input remains missing, rather than being inserted after simulation began.
        let incomplete_end = (rendered || refused_binding)
            && ((stage == SIMULATION_END && matches!(mask, BEGUN | BEGUN_SIMULATION))
                || (stage == FRAME_END && matches!(mask, BEGUN_SIM_END | BEGUN_SIM_INTERVAL)));
        if !rendered
            && event.flags & DERIVED != 0
            && stage == SIMULATION_BEGIN
            && reservation.is_some_and(|entry| refused_binding || entry.poll_begin_qpc == 0)
        {
            return Err(0);
        }
        let early_simulation = canonical && stage == SIMULATION_BEGIN && mask == BEGUN;
        let input_after_begin = canonical && stage == INPUT_SAMPLE && mask == BEGUN_SIMULATION;
        let previous = frame.timestamps_qpc.iter().copied().max().unwrap_or(0);
        if frame.qpc_frequency != event.qpc_frequency
            || event.timestamp_qpc < previous
            || (!incomplete_end
                && !partial_end
                && !early_simulation
                && !input_after_begin
                && mask != expected)
        {
            frame.failed = 1;
            totals.order_violations = totals.order_violations.wrapping_add(1);
            if stage == FRAME_END {
                frame.ended = 1;
                ledger.end(session, source);
            }
            return Err(0);
        }
        if stage == INPUT_SAMPLE && event.flags & SUBSTITUTE != 0 {
            frame.flags |= SUBSTITUTE;
            totals.substitutes = totals.substitutes.wrapping_add(1);
        }
        frame.timestamps_qpc[stage as usize] = event.timestamp_qpc;
        if event.flags & DERIVED != 0 {
            frame.flags |= DERIVED;
            slot.derived_stages |= stage_bit(stage);
        }
        frame.stage_mask |= stage_bit(stage);
        frame.ended = (stage == FRAME_END) as u32;
        if frame.ended != 0 {
            if event.flags & DERIVED != 0 {
                totals.derived_frame_ends = totals.derived_frame_ends.wrapping_add(1);
            }
            ledger.end(session, source);
            // The engine's own statement that this source will not present is a Present
            // outcome. CPU closure alone says nothing about Present.
            if event.flags & RENDER_SKIPPED != 0 && event.flags & DERIVED == 0 {
                ledger.present_result(
                    *current_session,
                    session,
                    source,
                    RSF_NATIVE_CPU_PRESENT_ABORTED,
                    0,
                );
            }
        }
        forward = !(refused_binding && stage != FRAME_BEGIN && stage != FRAME_END);
    }
    if !forward {
        return Ok((None, ptr::null_mut()));
    }
    Ok(state.route(sink, user))
}

/// Validates and records a CPU fact, then delivers it to the supplied sink, or to the
/// configured consumer when none is supplied.
///
/// # Safety
/// A non-null `event` must be readable through its advertised size. Any supplied
/// sink and user pointer must remain valid for the synchronous dispatch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_cpu_dispatch(
    event: *const Event,
    sink: Sink,
    user: *mut c_void,
) -> i32 {
    CPU.dispatch(event, sink, user)
}

/// Dispatches a fact to the configured consumer.
///
/// # Safety
/// A non-null `event` must be readable through its advertised size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_cpu_event(event: *const Event) -> i32 {
    CPU.dispatch(event, None, ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_rendered_source_set(enabled: u32) {
    CPU.rendered.store(enabled != 0, Ordering::SeqCst);
}

pub(crate) fn rendered_source() -> bool {
    CPU.rendered()
}

pub(crate) fn owns_session(session: u64) -> bool {
    session != 0 && CPU.lock().current_session == session
}

pub(crate) fn frame_facts(session: u64, source: u64) -> Option<rsf_native_cpu_frame> {
    CPU.read(session, source)
}

pub(crate) fn reservation(session: u64, source: u64) -> Option<rsf_native_cpu_reservation> {
    CPU.reservation(session, source)
}

pub(crate) fn counters() -> rsf_native_cpu_counters {
    CPU.lock().totals
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_cpu_read(
    session: u64,
    source: u64,
    frame: *mut rsf_native_cpu_frame,
) -> i32 {
    if !unsafe { has_capacity(frame) } {
        return 0;
    }
    match frame_facts(session, source) {
        Some(record) => {
            unsafe { frame.write(record) };
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_set_sink(sink: Sink, user: *mut c_void) {
    let mut state = CPU.lock();
    state.consumer = sink;
    state.consumer_user = user;
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_release() {
    *CPU.lock() = State::new();
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_release_session(session: u64) -> i32 {
    let mut state = CPU.lock();
    if session == 0 || state.current_session != session {
        return 0;
    }
    *state = State::new();
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_cpu_counters_get(out: *mut rsf_native_cpu_counters) -> i32 {
    if !unsafe { has_capacity(out) } {
        return 0;
    }
    unsafe { out.write(counters()) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_ping_message(message: u32) {
    CPU.lock().ping_message = if (0xc000..=0xffff).contains(&message) {
        message
    } else {
        0
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_marker(source: u64, marker: u32) -> i32 {
    mark_frame(None, source, marker)
}

pub(crate) fn marker_prerequisites(flags: u32, marker: u32) -> u32 {
    if flags & KEYED != 0 {
        (1 << marker) - 1
    } else {
        LEGACY_MARKER_REQUIRED[marker as usize]
    }
}

pub(crate) fn mark_frame(session: Option<u64>, source: u64, marker: u32) -> i32 {
    if source == 0 || marker > 5 {
        return -1;
    }
    let mut guard = CPU.lock();
    let state = &mut *guard;
    let current_session = state.current_session;
    if session.is_some_and(|session| session != current_session) {
        return -1;
    }
    let index = (source % FRAMES as u64) as usize;
    let CpuFrame {
        record: frame,
        derived_stages,
    } = &mut state.frames[index];
    if frame.source_frame_id != source
        || frame.session_id != current_session
        || (session.is_some() && frame.failed != 0)
    {
        return -1;
    }
    // Normalized bookkeeping is never authority for a vendor timing marker.
    if frame.flags & SUBSTITUTE != 0 {
        return -1;
    }
    if marker <= 1 {
        let stage = if marker == 0 {
            SIMULATION_BEGIN
        } else {
            SIMULATION_END
        };
        if frame.timestamps_qpc[stage as usize] == 0 || *derived_stages & stage_bit(stage) != 0 {
            return -1;
        }
    }
    if let Some(reservation) = state.ledger.get(current_session, source)
        && reservation.state != RSF_NATIVE_CPU_RESERVATION_BOUND
        && reservation.state != RSF_NATIVE_CPU_RESERVATION_CLOSED
    {
        return -1;
    }
    if frame.marker_mask & (1 << marker) != 0 {
        state.totals.duplicates = state.totals.duplicates.wrapping_add(1);
        return 0;
    }
    let required = marker_prerequisites(frame.flags, marker);
    if frame.marker_mask & required != required {
        state.totals.order_violations = state.totals.order_violations.wrapping_add(1);
        return -1;
    }
    frame.marker_mask |= 1 << marker;
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_cpu_reservation_read(
    session: u64,
    source: u64,
    out: *mut rsf_native_cpu_reservation,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return 0;
    }
    match reservation(session, source) {
        Some(reservation) => {
            unsafe { out.write(reservation) };
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_cpu_reservation_counters_get(
    out: *mut rsf_native_cpu_reservation_counters,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return 0;
    }
    unsafe { out.write(CPU.lock().ledger.counters()) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_present_result(
    session: u64,
    source: u64,
    outcome: u32,
    native_result: i32,
) -> i32 {
    let mut guard = CPU.lock();
    let state = &mut *guard;
    let current = state.current_session;
    state
        .ledger
        .present_result(current, session, source, outcome, native_result) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_paced(source: u64, began: u64, ready: u64) {
    record_pacing(None, source, began, ready);
}

pub(crate) fn record_pacing(session: Option<u64>, source: u64, began: u64, ready: u64) {
    if source == 0 || began == 0 || ready < began {
        return;
    }
    let mut guard = CPU.lock();
    let current_session = guard.current_session;
    if session.is_some_and(|session| session != current_session) {
        return;
    }
    let frame = &mut guard.slot(source).record;
    if frame.source_frame_id != source || frame.session_id != current_session {
        return;
    }
    frame.pacing_qpc = began;
    frame.input_ready_qpc = ready;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_native_input_receipts_get(out: *mut rsf_native_input_receipts) -> i32 {
    if !unsafe { has_capacity(out) } || unsafe { (*out).version } != 1 {
        return 0;
    }
    unsafe { out.write(CPU.lock().receipts) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_native_cpu_is_ping_message(message: u32) -> i32 {
    let state = CPU.lock();
    (state.ping_message != 0 && message == state.ping_message) as i32
}
