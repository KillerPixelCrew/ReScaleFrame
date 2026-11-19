// SPDX-License-Identifier: MIT
use crate::cpu_abi::*;
use rsf_game_sys::*;

type Reservation = rsf_native_cpu_reservation;
type Counters = rsf_native_cpu_reservation_counters;

const EMPTY: Reservation = unsafe { std::mem::zeroed() };
const EMPTY_COUNTERS: Counters = Counters {
    struct_size: size_of::<Counters>() as u32,
    ..unsafe { std::mem::zeroed() }
};

/// Prospective source reservations and predecessor Present outcomes. The native
/// CPU dispatcher owns it under its state lock; it never calls out.
pub(crate) struct Book {
    entries: [Reservation; 16],
    highest: u64,
    counters: Counters,
    outcomes: [(u64, i32); 64],
    next_outcome: usize,
    presented_through: u64,
}

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
    pub(crate) const fn new() -> Self {
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

    /// The stored entry, without the predecessor outcome.
    pub(crate) fn get(&self, session: u64, source: u64) -> Option<Reservation> {
        self.find(session, source).map(|index| self.entries[index])
    }

    /// The stored entry with its predecessor's Present outcome as known now.
    pub(crate) fn read(&self, session: u64, source: u64) -> Option<Reservation> {
        let mut reservation = self.get(session, source)?;
        reservation.predecessor_present = self.dependency(reservation.predecessor_source_id);
        Some(reservation)
    }

    pub(crate) fn counters(&self) -> Counters {
        self.counters
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

    /// A version 2 source fact. `None` is a malformed packet; every refusal counts.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn source_event(
        &mut self,
        event: Option<&rsf_game_cpu_source_event>,
        session: u64,
        frequency: u64,
        last_begin: u64,
        known_predecessor: bool,
        frame: &mut rsf_native_cpu_frame,
        rendered: bool,
    ) -> bool {
        if let Some(event) = event
            && valid(event, session, frequency)
            && self.event(event, last_begin, known_predecessor, frame, rendered)
        {
            return true;
        }
        self.counters.rejected = self.counters.rejected.wrapping_add(1);
        false
    }

    /// FRAME_BEGIN adopts a sampled reservation for its own identity and supersedes others.
    pub(crate) fn begin(&mut self, event: &rsf_game_cpu_event) {
        let Book {
            entries, counters, ..
        } = self;
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

    pub(crate) fn end(&mut self, session: u64, source: u64) {
        let Some(index) = self.find(session, source) else {
            return;
        };
        match self.entries[index].state {
            RSF_NATIVE_CPU_RESERVATION_ADOPTED => {
                self.entries[index].state = RSF_NATIVE_CPU_RESERVATION_UNBOUND;
                self.counters.unbound = self.counters.unbound.wrapping_add(1);
            }
            RSF_NATIVE_CPU_RESERVATION_BOUND => {
                self.entries[index].state = RSF_NATIVE_CPU_RESERVATION_CLOSED;
                self.counters.closed = self.counters.closed.wrapping_add(1);
            }
            _ => {}
        }
    }

    /// The first outcome reported for a source stands; later reports are refused.
    pub(crate) fn present_result(
        &mut self,
        current: u64,
        session: u64,
        source: u64,
        outcome: u32,
        native_result: i32,
    ) -> bool {
        if session == 0 || session != current || source == 0 {
            return false;
        }
        let state = match outcome {
            RSF_NATIVE_CPU_PRESENT_RETURNED if native_result >= 0 => {
                RSF_NATIVE_CPU_DEPENDENCY_PRESENTED
            }
            RSF_NATIVE_CPU_PRESENT_RETURNED => RSF_NATIVE_CPU_DEPENDENCY_FAILED,
            RSF_NATIVE_CPU_PRESENT_RESULT_UNKNOWN => RSF_NATIVE_CPU_DEPENDENCY_UNVERIFIED,
            RSF_NATIVE_CPU_PRESENT_ABORTED => RSF_NATIVE_CPU_DEPENDENCY_CANCELLED,
            _ => return false,
        };
        if self.outcomes.iter().any(|&(id, _)| id == source) {
            return false;
        }
        let index = self.next_outcome;
        self.outcomes[index] = (source, state as i32);
        self.next_outcome = (index + 1) % self.outcomes.len();
        if state == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED {
            self.presented_through = self.presented_through.max(source);
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
