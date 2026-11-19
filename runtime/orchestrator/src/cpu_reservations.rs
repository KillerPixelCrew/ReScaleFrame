// SPDX-License-Identifier: MIT
use crate::cpu_abi::*;
use rsf_game_sys::*;
use std::sync::Mutex;

type Reservation = rsf_native_cpu_reservation;
type Counters = rsf_native_cpu_reservation_counters;

const EMPTY: Reservation = unsafe { std::mem::zeroed() };
const EMPTY_COUNTERS: Counters = Counters {
    struct_size: size_of::<Counters>() as u32,
    ..unsafe { std::mem::zeroed() }
};

struct Book {
    entries: [Reservation; 16],
    highest: u64,
    counters: Counters,
    outcomes: [(u64, i32); 64],
    next_outcome: usize,
    presented_through: u64,
}

static BOOK: Mutex<Book> = Mutex::new(Book::new());

fn live(state: u32) -> bool {
    matches!(
        state,
        RSF_NATIVE_CPU_RESERVATION_OPEN | RSF_NATIVE_CPU_RESERVATION_SAMPLED
    )
}

fn owned(state: u32) -> bool {
    matches!(
        state,
        RSF_NATIVE_CPU_RESERVATION_ADOPTED | RSF_NATIVE_CPU_RESERVATION_BOUND
    )
}

fn abort(entry: &mut Reservation, reason: u32, counters: &mut Counters) {
    entry.state = RSF_NATIVE_CPU_RESERVATION_ABORTED;
    entry.abort_reason = reason;
    counters.aborted = counters.aborted.wrapping_add(1);
}

impl Book {
    const fn new() -> Self {
        Self {
            entries: [EMPTY; 16],
            highest: 0,
            counters: EMPTY_COUNTERS,
            outcomes: [(0, 0); 64],
            next_outcome: 0,
            presented_through: 0,
        }
    }

    fn find(&self, session: u64, source: u64) -> Option<usize> {
        if session == 0 || source == 0 {
            return None;
        }
        self.entries.iter().position(|entry| {
            entry.state != 0 && entry.session_id == session && entry.source_frame_id == source
        })
    }

    fn dependency(&self, source: u64) -> i32 {
        if source == 0 {
            return RSF_NATIVE_CPU_DEPENDENCY_NONE;
        }
        if let Some(&(_, state)) = self.outcomes.iter().find(|&&(id, _)| id == source) {
            return state;
        }
        if self.presented_through > source {
            RSF_NATIVE_CPU_DEPENDENCY_SUPERSEDED as i32
        } else {
            RSF_NATIVE_CPU_DEPENDENCY_PENDING as i32
        }
    }

    fn reserve(
        &mut self,
        event: &rsf_game_cpu_source_event,
        last_begin: u64,
        known_predecessor: bool,
    ) -> bool {
        let fact = &event.header;
        if fact.source_frame_id <= last_begin
            || fact.source_frame_id <= self.highest
            || !known_predecessor
        {
            return false;
        }
        self.highest = fact.source_frame_id;
        let mut overlap = false;
        for other in &mut self.entries {
            if other.session_id != fact.session_id || !live(other.state) {
                continue;
            }
            let polling = other.state == RSF_NATIVE_CPU_RESERVATION_OPEN;
            overlap |= polling;
            let reason = if polling {
                RSF_GAME_CPU_ABORT_OVERLAP
            } else {
                RSF_GAME_CPU_ABORT_INTERVENING_POLL
            };
            abort(other, reason, &mut self.counters);
        }
        if overlap {
            return false;
        }
        let Some(slot) = self
            .entries
            .iter_mut()
            .filter(|entry| !live(entry.state) && !owned(entry.state))
            .min_by_key(|entry| {
                if entry.state == 0 {
                    0
                } else {
                    entry.source_frame_id
                }
            })
        else {
            return false;
        };
        *slot = Reservation {
            struct_size: size_of::<Reservation>() as u32,
            state: RSF_NATIVE_CPU_RESERVATION_OPEN,
            session_id: fact.session_id,
            source_frame_id: fact.source_frame_id,
            predecessor_source_id: event.predecessor_source_id,
            native_generation: event.native_generation,
            qpc_frequency: fact.qpc_frequency,
            reserve_qpc: fact.timestamp_qpc,
            flags: fact.flags,
            ..EMPTY
        };
        self.counters.reserved = self.counters.reserved.wrapping_add(1);
        true
    }

    fn event(
        &mut self,
        event: &rsf_game_cpu_source_event,
        last_begin: u64,
        known_predecessor: bool,
        frame: &mut rsf_native_cpu_frame,
        rendered: bool,
    ) -> bool {
        let fact = &event.header;
        if fact.stage == RSF_GAME_CPU_SOURCE_RESERVE {
            return self.reserve(event, last_begin, known_predecessor);
        }
        let Some(index) = self.find(fact.session_id, fact.source_frame_id) else {
            return false;
        };
        let entry = &mut self.entries[index];
        if entry.qpc_frequency != fact.qpc_frequency {
            return false;
        }
        match fact.stage {
            RSF_GAME_CPU_SOURCE_SAMPLED => {
                if entry.state != RSF_NATIVE_CPU_RESERVATION_OPEN
                    || fact.timestamp_qpc < entry.reserve_qpc
                    || event.native_generation != entry.native_generation
                    || (event.poll_begin_qpc != 0 && event.poll_begin_qpc < entry.reserve_qpc)
                {
                    return false;
                }
                entry.state = RSF_NATIVE_CPU_RESERVATION_SAMPLED;
                entry.sampled_qpc = fact.timestamp_qpc;
                entry.poll_begin_qpc = event.poll_begin_qpc;
                entry.input_kind = fact.input_kind;
                entry.flags |= fact.flags;
                self.counters.sampled = self.counters.sampled.wrapping_add(1);
            }
            RSF_GAME_CPU_SOURCE_BIND => {
                if entry.state != RSF_NATIVE_CPU_RESERVATION_ADOPTED
                    || entry.flags & RSF_GAME_CPU_SOURCE_PRODUCED == 0
                    || frame.source_frame_id != fact.source_frame_id
                    || frame.session_id != fact.session_id
                    || frame.ended != 0
                    || frame.failed != 0
                    || fact.timestamp_qpc < entry.frame_begin_qpc
                    || fact.timestamp_qpc < entry.sampled_qpc
                {
                    return false;
                }
                let promote_poll = !rendered && entry.poll_begin_qpc != 0;
                if promote_poll && frame.stage_mask != 1 {
                    return false;
                }
                entry.state = RSF_NATIVE_CPU_RESERVATION_BOUND;
                entry.bind_qpc = fact.timestamp_qpc;
                if promote_poll {
                    // The verified poll predates FRAME_BEGIN. Bind proves which frame consumed it.
                    frame.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE as usize] = entry.poll_begin_qpc;
                    frame.stage_mask |= 1 << RSF_GAME_CPU_INPUT_SAMPLE;
                    frame.flags |= RSF_GAME_CPU_INPUT_KEYED;
                }
                self.counters.bound = self.counters.bound.wrapping_add(1);
            }
            RSF_GAME_CPU_SOURCE_ABORT if live(entry.state) || owned(entry.state) => {
                abort(entry, event.reason, &mut self.counters);
            }
            _ => return false,
        }
        true
    }
}

fn valid(event: &rsf_game_cpu_source_event, session: u64, frequency: u64) -> bool {
    let fact = &event.header;
    let allowed = match fact.stage {
        RSF_GAME_CPU_SOURCE_RESERVE => RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR,
        RSF_GAME_CPU_SOURCE_SAMPLED => RSF_GAME_CPU_SOURCE_PRODUCED,
        _ => 0,
    };
    session != 0
        && fact.session_id == session
        && fact.source_frame_id != 0
        && fact.timestamp_qpc != 0
        && fact.qpc_frequency != 0
        && fact.qpc_frequency == frequency
        && fact.flags & !allowed == 0
        && fact.message_id == 0
        && fact.render_expected == 0
        && fact.input_kind & !7 == 0
        && (fact.input_kind == 0 || fact.stage == RSF_GAME_CPU_SOURCE_SAMPLED)
        && event.reserved == 0
        && (event.reason == 0 || fact.stage == RSF_GAME_CPU_SOURCE_ABORT)
        && (event.predecessor_source_id == 0 || fact.stage == RSF_GAME_CPU_SOURCE_RESERVE)
        && (event.poll_begin_qpc == 0
            || (fact.stage == RSF_GAME_CPU_SOURCE_SAMPLED
                && event.poll_begin_qpc <= fact.timestamp_qpc))
}

// The C++ dispatcher keeps its frame lock across each of these calls. No
// runtime callback executes under the Rust reservation lock.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_cpu_source_event(
    event: *const rsf_game_cpu_source_event,
    session: u64,
    frequency: u64,
    last_begin: u64,
    known_predecessor: i32,
    frame: *mut rsf_native_cpu_frame,
    rendered: i32,
) -> i32 {
    let mut book = BOOK.lock().unwrap();
    if let Some(event) = unsafe { event.as_ref() }
        && valid(event, session, frequency)
        && book.event(
            event,
            last_begin,
            known_predecessor != 0,
            unsafe { &mut *frame },
            rendered != 0,
        )
    {
        return 1;
    }
    book.counters.rejected = book.counters.rejected.wrapping_add(1);
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_cpu_reservation_begin(event: *const rsf_game_cpu_event) {
    let event = unsafe { &*event };
    let mut book = BOOK.lock().unwrap();
    let Book {
        entries, counters, ..
    } = &mut *book;
    for entry in entries
        .iter_mut()
        .filter(|entry| entry.state != 0 && entry.session_id == event.session_id)
    {
        if entry.source_frame_id == event.source_frame_id {
            if entry.state == RSF_NATIVE_CPU_RESERVATION_SAMPLED
                && entry.qpc_frequency == event.qpc_frequency
                && event.timestamp_qpc >= entry.sampled_qpc
            {
                entry.state = RSF_NATIVE_CPU_RESERVATION_ADOPTED;
                entry.frame_begin_qpc = event.timestamp_qpc;
                counters.adopted = counters.adopted.wrapping_add(1);
            } else if live(entry.state) {
                abort(entry, RSF_GAME_CPU_ABORT_OVERLAP, counters);
            }
        } else if live(entry.state) {
            abort(entry, RSF_GAME_CPU_ABORT_SUPERSEDED, counters);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_cpu_reservation_end(session: u64, source: u64) {
    let mut book = BOOK.lock().unwrap();
    let Some(index) = book.find(session, source) else {
        return;
    };
    match book.entries[index].state {
        RSF_NATIVE_CPU_RESERVATION_ADOPTED => {
            book.entries[index].state = RSF_NATIVE_CPU_RESERVATION_UNBOUND;
            book.counters.unbound = book.counters.unbound.wrapping_add(1);
        }
        RSF_NATIVE_CPU_RESERVATION_BOUND => {
            book.entries[index].state = RSF_NATIVE_CPU_RESERVATION_CLOSED;
            book.counters.closed = book.counters.closed.wrapping_add(1);
        }
        _ => {}
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_cpu_reservation_read(
    session: u64,
    source: u64,
    out: *mut Reservation,
    include_dependency: i32,
) -> i32 {
    let book = BOOK.lock().unwrap();
    let Some(index) = book.find(session, source) else {
        return 0;
    };
    let mut reservation = book.entries[index];
    if include_dependency != 0 {
        reservation.predecessor_present = book.dependency(reservation.predecessor_source_id);
    }
    unsafe { out.write(reservation) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_cpu_reservation_counters(out: *mut Counters) {
    unsafe { out.write(BOOK.lock().unwrap().counters) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_cpu_present_result(
    current: u64,
    session: u64,
    source: u64,
    outcome: u32,
    native_result: i32,
) -> i32 {
    if session == 0 || session != current || source == 0 {
        return 0;
    }
    let state = match outcome {
        RSF_NATIVE_CPU_PRESENT_RETURNED if native_result >= 0 => {
            RSF_NATIVE_CPU_DEPENDENCY_PRESENTED
        }
        RSF_NATIVE_CPU_PRESENT_RETURNED => RSF_NATIVE_CPU_DEPENDENCY_FAILED,
        RSF_NATIVE_CPU_PRESENT_RESULT_UNKNOWN => RSF_NATIVE_CPU_DEPENDENCY_UNVERIFIED,
        RSF_NATIVE_CPU_PRESENT_ABORTED => RSF_NATIVE_CPU_DEPENDENCY_CANCELLED,
        _ => return 0,
    };
    let mut book = BOOK.lock().unwrap();
    if book.outcomes.iter().any(|&(id, _)| id == source) {
        return 0;
    }
    let index = book.next_outcome;
    book.outcomes[index] = (source, state as i32);
    book.next_outcome = (index + 1) % book.outcomes.len();
    if state == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED {
        book.presented_through = book.presented_through.max(source);
    }
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_cpu_reservations_clear() {
    *BOOK.lock().unwrap() = Book::new();
}
