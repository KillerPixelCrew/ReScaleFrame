// SPDX-License-Identifier: MIT
//! The bounded worker successor handoff.
//!
//! A selected mode1 worker poll reserves one future source while Main still owns the current
//! render. The ticket is one copied future intention, not a frame history. Its caller serializes
//! these scalar transitions under one lock and never holds that lock while delivering events.

/// The CPU source identity borrowed during the guarded Swap.
pub(crate) use crate::sys::rsf_fallout4_cpu_identity as CpuIdentity;

impl CpuIdentity {
    pub const ZERO: Self = Self {
        session: 0,
        source: 0,
        submission: 0,
        began_qpc: 0,
        frequency: 0,
    };
}

/// Poll generation belongs to a bound sample, not an input-unbound render association.
/// `poll_only` requires unchanged native owner/state and no lifecycle, raw-writer or overlapping
/// poll refusal; every other refusal invalidates both.
pub(crate) const fn invalidates_render_source(bound_source: u64, poll_only: bool) -> bool {
    bound_source != 0 || !poll_only
}

/// The native worker cycle that owns one render source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WorkerCycle {
    pub session: u64,
    pub source: u64,
    pub idle: u64,
    pub main: usize,
    pub worker: usize,
    pub window: usize,
    pub manager: usize,
    pub queue: usize,
    pub receiver: usize,
    pub start_event: usize,
    pub complete_event: usize,
    pub frame: u32,
}

impl WorkerCycle {
    pub const ZERO: Self = Self {
        session: 0,
        source: 0,
        idle: 0,
        main: 0,
        worker: 0,
        window: 0,
        manager: 0,
        queue: 0,
        receiver: 0,
        start_event: 0,
        complete_event: 0,
        frame: 0,
    };
}

/// Called with the current render owner locked after a worker saw mode0.
#[allow(clippy::too_many_arguments)]
pub(crate) fn active_join_matches(
    expected: &WorkerCycle,
    current: &WorkerCycle,
    predecessor: &WorkerCycle,
    worker_source: u64,
    successor_source: u64,
    joining: bool,
    swapped: bool,
    closed: bool,
    invalid: bool,
) -> bool {
    expected.source != 0
        && worker_source != 0
        && worker_source == successor_source
        && current == expected
        && predecessor == expected
        && joining
        && swapped
        && closed
        && !invalid
}

/// Same native owner, ignoring the source, idle serial and native frame.
pub(crate) fn same_worker_owner(a: &WorkerCycle, b: &WorkerCycle) -> bool {
    a.session == b.session
        && a.main == b.main
        && a.worker == b.worker
        && a.window == b.window
        && a.manager == b.manager
        && a.queue == b.queue
        && a.receiver == b.receiver
        && a.start_event == b.start_event
        && a.complete_event == b.complete_event
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Phase {
    Empty,
    Reserved,
    Armed,
    Sampled,
    Begun,
    Bound,
    Consumed,
    Complete,
    Joined,
    Claimed,
    Adopted,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkerTicket {
    pub phase: Phase,
    pub predecessor: WorkerCycle,
    pub cpu: CpuIdentity,
    pub generation: u64,
    pub raw: u64,
    pub poll_begin: u64,
    pub cursor: u32,
    pub refusal: u32,
    pub frame_begun: bool,
    pub callback_inflight: bool,
    pub callback_returned: bool,
    pub native_poll_started: bool,
    pub predecessor_generation: u64,
    pub predecessor_cursor: u32,
}

impl WorkerTicket {
    pub const ZERO: Self = Self {
        phase: Phase::Empty,
        predecessor: WorkerCycle::ZERO,
        cpu: CpuIdentity::ZERO,
        generation: 0,
        raw: 0,
        poll_begin: 0,
        cursor: 0,
        refusal: 0,
        frame_begun: false,
        callback_inflight: false,
        callback_returned: false,
        native_poll_started: false,
        predecessor_generation: 0,
        predecessor_cursor: 0,
    };

    pub fn active(&self) -> bool {
        self.phase != Phase::Empty
    }

    pub fn valid(&self) -> bool {
        self.active() && self.refusal == 0
    }

    /// The first refusal is sticky.
    pub fn refuse(&mut self, reason: u32) {
        if self.active() && self.refusal == 0 {
            self.refusal = reason;
        }
    }

    pub fn reserve(&mut self, owner: &WorkerCycle, future: &CpuIdentity, generation: u64) -> bool {
        if self.active()
            || owner.source == 0
            || future.source <= owner.source
            || future.session != owner.session
            || generation == 0
        {
            return false;
        }
        self.predecessor = *owner;
        self.cpu = *future;
        self.generation = generation;
        self.phase = Phase::Reserved;
        true
    }

    pub fn begin_callback(&mut self, bound_generation: u64, bound_cursor: u32) -> bool {
        if !self.valid()
            || self.phase != Phase::Reserved
            || self.callback_inflight
            || self.callback_returned
            || self.native_poll_started
            || (bound_generation != 0
                && (self.generation <= bound_generation || self.generation - bound_generation != 1))
        {
            return false;
        }
        self.predecessor_generation = bound_generation;
        self.predecessor_cursor = bound_cursor;
        self.callback_inflight = true;
        true
    }

    pub fn end_callback(&mut self) {
        self.callback_returned = self.callback_inflight;
        self.callback_inflight = false;
    }

    /// The predecessor's sample is already consumed. Raw arrivals and queue-tail writes belong to
    /// the future poll, which has not started during this lease.
    pub fn protects_bound(
        &self,
        owner: &WorkerCycle,
        source: u64,
        bound_generation: u64,
        generation: u64,
        consumed: u32,
        native_valid: bool,
    ) -> bool {
        self.valid()
            && self.phase == Phase::Reserved
            && self.callback_inflight
            && !self.native_poll_started
            && *owner == self.predecessor
            && source == self.predecessor.source
            && self.predecessor_generation != 0
            && bound_generation == self.predecessor_generation
            && generation == self.generation
            && self.generation > bound_generation
            && self.generation - bound_generation == 1
            && consumed == self.predecessor_cursor
            && native_valid
    }

    pub const fn poll_safe(bound_source: u64, closed: bool, sealed: bool) -> bool {
        bound_source == 0 || closed || sealed
    }

    pub fn arm(
        &mut self,
        owner: &WorkerCycle,
        generation: u64,
        revision: u64,
        predecessor_safe: bool,
    ) -> bool {
        if !self.valid()
            || self.phase != Phase::Reserved
            || self.callback_inflight
            || !self.callback_returned
            || *owner != self.predecessor
            || generation != self.generation
            || !predecessor_safe
        {
            return false;
        }
        self.raw = revision;
        self.phase = Phase::Armed;
        true
    }

    pub fn native_poll_entry(&mut self) {
        self.callback_inflight = false;
        self.native_poll_started = true;
    }

    pub fn sample(
        &mut self,
        generation: u64,
        revision: u64,
        sequence: u32,
        began: u64,
        native_valid: bool,
    ) -> bool {
        if !self.valid()
            || self.phase != Phase::Armed
            || !self.native_poll_started
            || generation != self.generation
            || revision != self.raw
            || !native_valid
        {
            return false;
        }
        self.cursor = sequence;
        self.poll_begin = began;
        self.phase = Phase::Sampled;
        true
    }

    pub fn begin(&mut self, now: u64, generation: u64, revision: u64, sequence: u32) -> bool {
        if !self.valid()
            || self.phase != Phase::Sampled
            || generation != self.generation
            || revision != self.raw
            || sequence != self.cursor
        {
            return false;
        }
        self.cpu.began_qpc = now;
        self.frame_begun = true;
        self.phase = Phase::Begun;
        true
    }

    pub fn bind(&mut self, generation: u64, revision: u64, sequence: u32) -> bool {
        if !self.valid()
            || self.phase != Phase::Begun
            || generation != self.generation
            || revision != self.raw
            || sequence != self.cursor
        {
            return false;
        }
        self.phase = Phase::Bound;
        true
    }

    pub fn consumed(&mut self, generation: u64, sequence: u32) -> bool {
        if !self.valid()
            || self.phase != Phase::Bound
            || generation != self.generation
            || sequence != self.cursor
        {
            return false;
        }
        self.phase = Phase::Consumed;
        true
    }

    pub fn complete(&mut self) -> bool {
        if !self.valid() || self.phase != Phase::Consumed {
            return false;
        }
        self.phase = Phase::Complete;
        true
    }

    pub fn join(&mut self, owner: &WorkerCycle) -> bool {
        if !self.valid() || self.phase != Phase::Complete || *owner != self.predecessor {
            return false;
        }
        self.phase = Phase::Joined;
        true
    }

    /// `sequence` is the receiver consumed cursor, never the live enqueue tail.
    pub fn successor(
        &self,
        owner: &WorkerCycle,
        sequence: u32,
        generation: u64,
        world: bool,
    ) -> bool {
        self.valid()
            && same_worker_owner(owner, &self.predecessor)
            && owner.source == self.cpu.source
            && owner.idle == self.predecessor.idle.wrapping_add(1)
            && owner.frame == self.predecessor.frame.wrapping_add(u32::from(world))
            && sequence == self.cursor
            && generation == self.generation
    }

    pub fn claim(&mut self, owner: &WorkerCycle, sequence: u32, generation: u64) -> bool {
        if self.phase != Phase::Joined || !self.successor(owner, sequence, generation, false) {
            return false;
        }
        self.phase = Phase::Claimed;
        true
    }

    pub fn adopt(&mut self, owner: &WorkerCycle, sequence: u32, generation: u64) -> bool {
        if self.phase != Phase::Claimed || !self.successor(owner, sequence, generation, true) {
            return false;
        }
        self.phase = Phase::Adopted;
        true
    }
}

#[cfg(test)]
mod tests;
