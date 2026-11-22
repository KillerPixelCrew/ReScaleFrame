// SPDX-License-Identifier: MIT
//! Native job and input producer proof, plus the two prospective source slots. The
//! detours lend their thread-local scopes and make the native calls at their original sites.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard, PoisonError};

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct JobTicket {
    pub(crate) generation: u64,
    pub(crate) source: u64,
    pub(crate) session: u64,
    pub(crate) manager: usize,
}

pub(crate) struct NativeJobs {
    state: AtomicU64,
    next: AtomicU64,
    source: AtomicU64,
    session: AtomicU64,
    retired: AtomicU64,
    manager: AtomicUsize,
}

impl JobTicket {
    pub(crate) const EMPTY: Self = Self {
        generation: 0,
        source: 0,
        session: 0,
        manager: 0,
    };
}

impl NativeJobs {
    const PUBLISHING: u64 = 1;
    const POISONED: u64 = 3;
    const GENERATION_STRIDE: u64 = 4;

    const fn new() -> Self {
        Self {
            state: AtomicU64::new(0),
            next: AtomicU64::new(1),
            source: AtomicU64::new(0),
            session: AtomicU64::new(0),
            retired: AtomicU64::new(0),
            manager: AtomicUsize::new(0),
        }
    }

    fn claim(&self, expected: u64) -> Result<u64, u64> {
        self.state
            .compare_exchange(expected, Self::PUBLISHING, SeqCst, SeqCst)
    }

    pub(crate) fn publish(
        &self,
        manager: usize,
        source: u64,
        session: u64,
        native_idle: bool,
    ) -> JobTicket {
        if manager == 0 || source == 0 || session == 0 {
            return JobTicket::default();
        }
        // Verified native idle can recover poison. A live or publishing job
        // still refuses the new ticket.
        if let Err(state) = self.claim(0)
            && (!native_idle || state != Self::POISONED || self.claim(Self::POISONED).is_err())
        {
            self.invalidate();
            return JobTicket::default();
        }
        // A worker can run as soon as the native original wakes the mode-1 manager.
        let generation = self
            .next
            .fetch_add(1, SeqCst)
            .wrapping_mul(Self::GENERATION_STRIDE);
        self.manager.store(manager, SeqCst);
        self.source.store(source, SeqCst);
        self.session.store(session, SeqCst);
        if self
            .state
            .compare_exchange(Self::PUBLISHING, generation, SeqCst, SeqCst)
            .is_err()
        {
            return JobTicket::default();
        }
        JobTicket {
            generation,
            source,
            session,
            manager,
        }
    }

    pub(crate) fn snapshot(&self) -> JobTicket {
        let generation = self.state.load(SeqCst);
        if generation < Self::GENERATION_STRIDE {
            return JobTicket::default();
        }
        let ticket = JobTicket {
            generation,
            source: self.source.load(SeqCst),
            session: self.session.load(SeqCst),
            manager: self.manager.load(SeqCst),
        };
        if self.state.load(SeqCst) == generation {
            ticket
        } else {
            JobTicket::default()
        }
    }

    pub(crate) fn matches(&self, ticket: JobTicket) -> bool {
        let current = self.snapshot();
        ticket.generation != 0 && current == ticket
    }

    pub(crate) fn retire(&self, ticket: JobTicket) -> bool {
        if !self.matches(ticket)
            || self
                .state
                .compare_exchange(ticket.generation, 0, SeqCst, SeqCst)
                .is_err()
        {
            return false;
        }
        self.retired.store(ticket.generation, SeqCst);
        true
    }

    pub(crate) fn invalidate(&self) {
        self.state.store(Self::POISONED, SeqCst);
        self.retired.store(0, SeqCst);
    }

    pub(crate) fn retired(&self) -> u64 {
        self.retired.load(SeqCst)
    }

    // The native owner verifies that publishing and observing hooks have drained.
    fn reset(&self) {
        self.state.store(0, SeqCst);
        self.retired.store(0, SeqCst);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Stamp {
    revision: u64,
    pub(crate) generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct PollTicket {
    pub(crate) generation: u64,
    conflicts: u64,
    exclusive: bool,
}

pub(crate) struct InputEpochs {
    publication: Mutex<()>,
    next: AtomicU64,
    revision: AtomicU64,
    completed: AtomicU64,
    conflicts: AtomicU64,
    writers: AtomicU32,
    polls: AtomicU32,
}

impl InputEpochs {
    const fn new() -> Self {
        Self {
            publication: Mutex::new(()),
            next: AtomicU64::new(1),
            revision: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            conflicts: AtomicU64::new(0),
            writers: AtomicU32::new(0),
            polls: AtomicU32::new(0),
        }
    }

    pub(crate) fn begin_poll(&self) -> PollTicket {
        let _guard = self
            .publication
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.begin_write();
        let conflicts = self.conflicts.load(SeqCst);
        let exclusive = self.polls.fetch_add(1, SeqCst) == 0;
        if !exclusive {
            self.conflicts.fetch_add(1, SeqCst);
        }
        PollTicket {
            generation: self.next.fetch_add(1, SeqCst),
            conflicts,
            exclusive,
        }
    }

    pub(crate) fn end_poll(&self, poll: PollTicket, produced: bool) {
        self.publish_poll(poll, produced, || {});
    }

    fn publish_poll(&self, poll: PollTicket, produced: bool, before_publish: impl FnOnce()) {
        let _guard = self
            .publication
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let completed =
            if produced && poll.exclusive && poll.conflicts == self.conflicts.load(SeqCst) {
                poll.generation
            } else {
                0
            };
        before_publish();
        self.completed.store(completed, SeqCst);
        self.polls.fetch_sub(1, SeqCst);
        self.end_write();
    }

    fn owns_poll(&self, poll: PollTicket) -> bool {
        let _guard = self
            .publication
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        poll.generation != 0
            && poll.exclusive
            && self.polls.load(SeqCst) == 1
            && poll.conflicts == self.conflicts.load(SeqCst)
    }

    pub(crate) fn invalidate(&self) {
        let _guard = self
            .publication
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.begin_write();
        self.conflicts.fetch_add(1, SeqCst);
        self.completed.store(0, SeqCst);
        self.end_write();
    }

    pub(crate) fn begin_write(&self) {
        self.writers.fetch_add(1, SeqCst);
        self.revision.fetch_add(1, SeqCst);
    }

    pub(crate) fn end_write(&self) {
        self.revision.fetch_add(1, SeqCst);
        self.writers.fetch_sub(1, SeqCst);
    }

    pub(crate) fn snapshot(&self) -> Stamp {
        let stamp = Stamp {
            revision: self.revision.load(SeqCst),
            generation: self.completed.load(SeqCst),
        };
        if self.unchanged(stamp) {
            stamp
        } else {
            Stamp::default()
        }
    }

    pub(crate) fn unchanged(&self, stamp: Stamp) -> bool {
        stamp.generation != 0
            && self.writers.load(SeqCst) == 0
            && self.revision.load(SeqCst) == stamp.revision
    }
}

pub(crate) static JOBS: NativeJobs = NativeJobs::new();
pub(crate) static EPOCHS: InputEpochs = InputEpochs::new();

#[repr(u32)]
#[derive(Clone, Copy)]
pub(crate) enum Consumer {
    Movement,
    PlayerUpdate,
    Camera,
    PlayerCamera,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct PollScope {
    pub(crate) ticket: PollTicket,
    pub(crate) manager: usize,
    pub(crate) produced: bool,
    pub(crate) kinds: u32,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct PollProvenance {
    pub(crate) session: u64,
    pub(crate) job_generation: u64,
    pub(crate) submitting_source: u64,
    pub(crate) poll_generation: u64,
}

pub(crate) fn rsf_skyrim_poll_provenance(
    out: &mut PollProvenance,
    scope: &PollScope,
    ticket: &JobTicket,
) -> bool {
    // Read live manager fields between the two proof checks. These guarded observations
    // make no engine or host calls.
    if !EPOCHS.owns_poll(scope.ticket)
        || !JOBS.matches(*ticket)
        || !crate::cpu_native::poll_managers_match(ticket.manager, scope.manager)
        || !JOBS.matches(*ticket)
        || !EPOCHS.owns_poll(scope.ticket)
    {
        return false;
    }
    *out = PollProvenance {
        session: ticket.session,
        job_generation: ticket.generation,
        submitting_source: ticket.source,
        poll_generation: scope.ticket.generation,
    };
    true
}

const _: () = {
    assert!(size_of::<JobTicket>() == 32);
    assert!(std::mem::offset_of!(JobTicket, manager) == 24);
    assert!(size_of::<Stamp>() == 16);
    assert!(size_of::<PollTicket>() == 24);
    assert!(std::mem::offset_of!(PollTicket, exclusive) == 16);
    assert!(size_of::<PollScope>() == 40);
    assert!(std::mem::offset_of!(PollScope, kinds) == 36);
    assert!(size_of::<PollProvenance>() == 32);
    assert!(std::mem::offset_of!(PollProvenance, poll_generation) == 24);
};

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    None,
    Polling,
    Sampled,
    Adopted,
    Bound,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Reservation {
    pub(crate) source: u64,
    pub(crate) predecessor: u64,
    pub(crate) session: u64,
    pub(crate) job_generation: u64,
    pub(crate) poll_generation: u64,
    pub(crate) phase: Phase,
    pub(crate) produced: bool,
    pub(crate) kinds: u32,
}

impl Reservation {
    pub(crate) const EMPTY: Self = Self {
        source: 0,
        predecessor: 0,
        session: 0,
        job_generation: 0,
        poll_generation: 0,
        phase: Phase::None,
        produced: false,
        kinds: 0,
    };

    fn polling(self, source: u64) -> bool {
        source != 0 && self.source == source && self.phase == Phase::Polling
    }
}

#[repr(C)]
pub(crate) struct Reserve {
    pub(crate) reserved: Reservation,
    pub(crate) withdrawn: u64,
    pub(crate) overlap: bool,
}

struct Sources {
    pending: Reservation,
    owned: Reservation,
}

impl Sources {
    const EMPTY: Self = Self {
        pending: Reservation::EMPTY,
        owned: Reservation::EMPTY,
    };

    fn reserve(&mut self, reservation: Reservation) -> Reserve {
        let mut result = Reserve {
            reserved: Reservation::EMPTY,
            withdrawn: 0,
            overlap: false,
        };
        if reservation.source == 0
            || reservation.session == 0
            || reservation.job_generation == 0
            || reservation.poll_generation == 0
        {
            return result;
        }
        if self.pending.phase != Phase::None {
            result.withdrawn = self.pending.source;
            result.overlap = self.pending.phase == Phase::Polling;
            self.pending = Reservation::EMPTY;
            if result.overlap {
                return result;
            }
        }
        self.pending = reservation;
        result.reserved = reservation;
        result
    }

    fn sample(&mut self, source: u64, produced: bool, kinds: u32) -> bool {
        if !self.pending.polling(source) {
            return false;
        }
        self.pending.phase = Phase::Sampled;
        self.pending.produced = produced;
        self.pending.kinds = kinds;
        true
    }

    fn withdraw_pending(&mut self, only: u64) -> u64 {
        if self.pending.phase == Phase::None || (only != 0 && self.pending.source != only) {
            return 0;
        }
        std::mem::replace(&mut self.pending, Reservation::EMPTY).source
    }

    fn adopt(&mut self, session: u64, joined_job_generation: u64) -> Reservation {
        if self.owned.phase != Phase::None
            || self.pending.phase != Phase::Sampled
            || !self.pending.produced
            || self.pending.session != session
            || joined_job_generation == 0
            || self.pending.job_generation != joined_job_generation
        {
            return Reservation::EMPTY;
        }
        self.owned = std::mem::replace(&mut self.pending, Reservation::EMPTY);
        self.owned.phase = Phase::Adopted;
        self.owned
    }

    fn bind(&mut self, source: u64, completed_poll_generation: u64) -> bool {
        if source == 0
            || self.owned.source != source
            || self.owned.phase != Phase::Adopted
            || completed_poll_generation != self.owned.poll_generation
        {
            return false;
        }
        self.owned.phase = Phase::Bound;
        true
    }

    fn release(&mut self, source: u64) -> Reservation {
        if source == 0 || self.owned.source != source {
            return Reservation::EMPTY;
        }
        std::mem::replace(&mut self.owned, Reservation::EMPTY)
    }
}

type Store = Mutex<Sources>;
static PRODUCTION: Store = Mutex::new(Sources::EMPTY);

fn lock_store(store: &Store) -> MutexGuard<'_, Sources> {
    store.lock().unwrap_or_else(PoisonError::into_inner)
}

#[repr(u32)]
pub(crate) enum PollSample {
    None,
    NotProduced,
    Ready,
}

pub(crate) fn rsf_skyrim_poll_sample_status(scope: &PollScope, source: u64) -> PollSample {
    // Finish an exclusive poll first. Report SAMPLED only for the exact completed
    // generation, and leave it unadoptable until the native host call returns.
    if EPOCHS.snapshot().generation != scope.ticket.generation {
        if lock_store(&PRODUCTION).withdraw_pending(source) != 0 {
            PollSample::NotProduced
        } else {
            PollSample::None
        }
    } else if lock_store(&PRODUCTION).pending.polling(source) {
        PollSample::Ready
    } else {
        PollSample::None
    }
}

const _: () = {
    assert!(size_of::<Reservation>() == 56);
    assert!(std::mem::offset_of!(Reservation, phase) == 40);
    assert!(std::mem::offset_of!(Reservation, kinds) == 48);
    assert!(size_of::<Reserve>() == 72);
};

pub(crate) fn rsf_skyrim_sources_reserve(
    out: &mut Reserve,
    source: u64,
    predecessor: u64,
    session: u64,
    job_generation: u64,
    poll_generation: u64,
) {
    *out = lock_store(&PRODUCTION).reserve(Reservation {
        source,
        predecessor,
        session,
        job_generation,
        poll_generation,
        phase: Phase::Polling,
        ..Reservation::EMPTY
    });
}

pub(crate) fn rsf_skyrim_sources_still_reserved(source: u64) -> bool {
    lock_store(&PRODUCTION).pending.polling(source)
}

pub(crate) fn rsf_skyrim_sources_sample(source: u64, produced: bool, kinds: u32) -> bool {
    lock_store(&PRODUCTION).sample(source, produced, kinds)
}

pub(crate) fn rsf_skyrim_sources_withdraw_pending(only: u64) -> u64 {
    lock_store(&PRODUCTION).withdraw_pending(only)
}

pub(crate) fn rsf_skyrim_sources_adopt(out: &mut Reservation, session: u64, joined: u64) {
    *out = lock_store(&PRODUCTION).adopt(session, joined);
}

pub(crate) fn rsf_skyrim_sources_bind(source: u64, completed: u64) -> bool {
    lock_store(&PRODUCTION).bind(source, completed)
}

pub(crate) fn rsf_skyrim_sources_owned(out: &mut Reservation) {
    *out = lock_store(&PRODUCTION).owned;
}

pub(crate) fn rsf_skyrim_sources_release(out: &mut Reservation, source: u64) {
    *out = lock_store(&PRODUCTION).release(source);
}

// Counts of observed native call envelopes for diagnostics. These never reach
// GameSDK as consumption or establish full movement/camera coverage.
struct WorkerConsumption {
    source: AtomicU64,
    movement: AtomicU32,
    player_update: AtomicU32,
    camera: AtomicU32,
    mismatched: AtomicU32,
}

static WORKERS: WorkerConsumption = WorkerConsumption {
    source: AtomicU64::new(0),
    movement: AtomicU32::new(0),
    player_update: AtomicU32::new(0),
    camera: AtomicU32::new(0),
    mismatched: AtomicU32::new(0),
};

#[repr(C)]
#[derive(Default)]
pub(crate) struct WorkerCounts {
    pub(crate) movement: u32,
    pub(crate) player_update: u32,
    pub(crate) camera: u32,
    pub(crate) mismatched: u32,
}

const _: () = assert!(size_of::<WorkerCounts>() == 16);

pub(crate) fn rsf_skyrim_worker_bind(source: u64) {
    WORKERS.source.store(0, SeqCst);
    WORKERS.movement.store(0, SeqCst);
    WORKERS.player_update.store(0, SeqCst);
    WORKERS.camera.store(0, SeqCst);
    WORKERS.mismatched.store(0, SeqCst);
    WORKERS.source.store(source, SeqCst);
}

pub(crate) fn rsf_skyrim_worker_release() {
    WORKERS.source.store(0, SeqCst);
}

pub(crate) fn rsf_skyrim_worker_intervening_poll() {
    if WORKERS.source.load(SeqCst) != 0 {
        WORKERS.mismatched.fetch_add(1, SeqCst);
    }
}

pub(crate) fn rsf_skyrim_worker_record_consumer(stamp: &Stamp, consumer: Consumer) {
    let source = WORKERS.source.load(SeqCst);
    if source == 0 {
        return;
    }
    let job = JOBS.snapshot();
    let owned = lock_store(&PRODUCTION).owned;
    if job.generation == 0
        || job.source != source
        || owned.source != source
        || owned.phase != Phase::Bound
    {
        return;
    }
    let count = match consumer {
        Consumer::Movement => &WORKERS.movement,
        Consumer::PlayerUpdate => &WORKERS.player_update,
        Consumer::Camera => &WORKERS.camera,
        Consumer::PlayerCamera => return,
    };
    if stamp.generation == owned.poll_generation && EPOCHS.unchanged(*stamp) {
        count.fetch_add(1, SeqCst);
    } else {
        WORKERS.mismatched.fetch_add(1, SeqCst);
    }
}

pub(crate) fn rsf_skyrim_worker_counts(out: &mut WorkerCounts) {
    *out = WorkerCounts {
        movement: WORKERS.movement.load(SeqCst),
        player_update: WORKERS.player_update.load(SeqCst),
        camera: WORKERS.camera.load(SeqCst),
        mismatched: WORKERS.mismatched.load(SeqCst),
    };
}

pub(crate) fn clear_proof(drained: bool) {
    if drained {
        JOBS.reset();
    } else {
        JOBS.invalidate();
    }
    EPOCHS.invalidate();
    *lock_store(&PRODUCTION) = Sources::EMPTY;
    rsf_skyrim_worker_release();
}

#[repr(u32)]
enum BindRefusal {
    None,
    Unreadable,
    QueuedMove,
    ConsumedBeforeBind,
    FreeCamera,
}

pub(crate) fn rsf_skyrim_bind_refusal(
    readable: bool,
    queued_move: bool,
    consumed_before_bind: bool,
    free_camera: bool,
) -> u32 {
    let reason = if !readable {
        BindRefusal::Unreadable
    } else if queued_move {
        BindRefusal::QueuedMove
    } else if consumed_before_bind {
        BindRefusal::ConsumedBeforeBind
    } else if free_camera {
        BindRefusal::FreeCamera
    } else {
        BindRefusal::None
    };
    reason as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_proof_survives_only_the_matching_native_join() {
        let jobs = NativeJobs::new();
        assert_eq!(jobs.snapshot().generation, 0);
        assert_eq!(jobs.publish(0, 11, 22, false).generation, 0);
        let first = jobs.publish(0x1234, 11, 22, false);
        assert!(jobs.matches(first));
        let foreign = JobTicket {
            source: 99,
            ..first
        };
        assert!(!jobs.matches(foreign) && !jobs.retire(foreign));
        assert!(jobs.retire(first));
        assert_eq!(jobs.snapshot().generation, 0);
        let next = jobs.publish(0x1234, 12, 22, false);
        assert!(next.generation > first.generation && !jobs.retire(first));
        assert_eq!(jobs.publish(0x1234, 13, 22, false).generation, 0);
        assert_eq!(jobs.snapshot().generation, 0);
        assert_eq!(jobs.publish(0x1234, 14, 22, false).generation, 0);
        let recovered = jobs.publish(0x1234, 14, 22, true);
        assert!(jobs.matches(recovered));
        assert_eq!(jobs.publish(0x1234, 15, 22, true).generation, 0);
        assert!(!jobs.matches(recovered));
        jobs.reset();
        let restarted = jobs.publish(0x1234, 15, 23, false);
        assert!(jobs.matches(restarted));
        jobs.invalidate();
        assert!(!jobs.matches(restarted) && !jobs.retire(restarted));
        assert_eq!(jobs.retired.load(SeqCst), 0);
        jobs.reset();
        let joined = jobs.publish(0x1234, 16, 23, false);
        assert!(jobs.retire(joined));
        assert_eq!(jobs.retired.load(SeqCst), joined.generation);
    }

    #[test]
    fn prospective_sources_require_matching_sample_join_and_bind() {
        let mut ledger = Sources::EMPTY;
        let reserve = |source, job_generation, poll_generation| Reservation {
            source,
            predecessor: source - 1,
            session: 23,
            job_generation,
            poll_generation,
            phase: Phase::Polling,
            ..Reservation::EMPTY
        };
        let first = ledger.reserve(reserve(17, 4, 5));
        assert_eq!((first.reserved.source, first.withdrawn), (17, 0));
        assert_eq!(ledger.adopt(23, 4).source, 0);
        assert!(ledger.pending.polling(17));
        assert!(ledger.sample(17, true, rsf_game_sys::RSF_GAME_INPUT_MOUSE));
        assert_eq!(ledger.adopt(24, 4).source, 0);
        assert_eq!(ledger.adopt(23, 8).source, 0);
        let adopted = ledger.adopt(23, 4);
        assert_eq!((adopted.source, adopted.predecessor), (17, 16));
        assert!(adopted.phase == Phase::Adopted);
        assert!(!ledger.bind(17, 6));
        assert!(ledger.bind(17, 5) && ledger.owned.phase == Phase::Bound);
        assert!(!ledger.bind(17, 5));
        let pending = ledger.reserve(reserve(18, 8, 6));
        assert_eq!(
            (
                pending.reserved.source,
                pending.withdrawn,
                ledger.owned.source
            ),
            (18, 0, 17)
        );
        assert!(ledger.release(17).phase == Phase::Bound);
        assert_eq!(ledger.owned.source, 0);
        let overlap = ledger.reserve(reserve(19, 8, 7));
        assert_eq!(
            (overlap.reserved.source, overlap.withdrawn, overlap.overlap),
            (0, 18, true)
        );
        ledger.reserve(reserve(20, 8, 8));
        assert!(ledger.sample(20, true, 0));
        let next = ledger.reserve(reserve(21, 8, 9));
        assert_eq!(
            (next.reserved.source, next.withdrawn, next.overlap),
            (21, 20, false)
        );
        assert_eq!(ledger.withdraw_pending(0), 21);
        assert_eq!(ledger.withdraw_pending(0), 0);
        ledger.reserve(reserve(22, 8, 10));
        assert_eq!(ledger.withdraw_pending(23), 0);
        assert_eq!(ledger.withdraw_pending(22), 22);
        ledger.reserve(reserve(24, 8, 11));
        assert!(ledger.sample(24, false, 0));
        assert_eq!(ledger.adopt(23, 8).source, 0);
        ledger = Sources::EMPTY;
        assert_eq!(ledger.withdraw_pending(0), 0);
        assert_eq!(ledger.owned.source, 0);
        assert_eq!(ledger.reserve(reserve(25, 0, 12)).reserved.source, 0);
        assert_eq!(ledger.reserve(reserve(25, 4, 0)).reserved.source, 0);
    }

    #[test]
    fn binding_requires_the_unconsumed_gameplay_envelope() {
        assert_eq!(
            rsf_skyrim_bind_refusal(true, false, false, false),
            BindRefusal::None as u32
        );
        assert_eq!(
            rsf_skyrim_bind_refusal(false, false, false, false),
            BindRefusal::Unreadable as u32
        );
        assert_eq!(
            rsf_skyrim_bind_refusal(true, true, false, false),
            BindRefusal::QueuedMove as u32
        );
        assert_eq!(
            rsf_skyrim_bind_refusal(true, false, true, false),
            BindRefusal::ConsumedBeforeBind as u32
        );
        assert_eq!(
            rsf_skyrim_bind_refusal(true, false, false, true),
            BindRefusal::FreeCamera as u32
        );
    }

    #[test]
    fn exclusive_poll_proof_is_lost_on_writes_overlap_and_invalidation() {
        let epochs = InputEpochs::new();
        assert_eq!(epochs.snapshot().generation, 0);
        let poll = epochs.begin_poll();
        assert!(epochs.owns_poll(poll));
        assert_eq!(epochs.snapshot().generation, 0);
        epochs.end_poll(poll, true);
        let consumed = epochs.snapshot();
        assert_eq!(consumed.generation, poll.generation);
        assert!(epochs.unchanged(consumed));
        let poll = epochs.begin_poll();
        assert!(!epochs.unchanged(consumed));
        epochs.end_poll(poll, false);
        assert_eq!(epochs.snapshot().generation, 0);
        for overlapping_finishes_first in [true, false] {
            let poll = epochs.begin_poll();
            let overlap = epochs.begin_poll();
            assert!(!epochs.owns_poll(poll) && !epochs.owns_poll(overlap));
            let (first, second) = if overlapping_finishes_first {
                (overlap, poll)
            } else {
                (poll, overlap)
            };
            epochs.end_poll(first, true);
            epochs.end_poll(second, true);
            assert_eq!(epochs.snapshot().generation, 0);
        }
        let poll = epochs.begin_poll();
        epochs.invalidate();
        epochs.end_poll(poll, true);
        assert_eq!(epochs.snapshot().generation, 0);
        let poll = epochs.begin_poll();
        epochs.end_poll(poll, true);
        let before_write = epochs.snapshot();
        epochs.begin_write();
        assert_eq!(epochs.snapshot().generation, 0);
        epochs.end_write();
        assert!(!epochs.unchanged(before_write));
        epochs.invalidate();
        assert_eq!(epochs.snapshot().generation, 0);
    }

    #[test]
    fn another_poll_cannot_enter_between_the_overlap_check_and_commit() {
        use std::sync::{Barrier, TryLockError};

        let epochs = InputEpochs::new();
        let first = epochs.begin_poll();
        let checked = Barrier::new(2);
        let release = Barrier::new(2);
        std::thread::scope(|threads| {
            threads.spawn(|| {
                epochs.publish_poll(first, true, || {
                    checked.wait();
                    release.wait();
                });
            });
            checked.wait();
            let could_enter =
                !matches!(epochs.publication.try_lock(), Err(TryLockError::WouldBlock));
            release.wait();
            assert!(!could_enter);
            let next = epochs.begin_poll();
            assert!(epochs.owns_poll(next));
            epochs.end_poll(next, false);
        });
        assert_eq!(epochs.snapshot().generation, 0);
    }

    #[test]
    fn a_completed_overlap_invalidates_the_first_poll() {
        let epochs = InputEpochs::new();
        let first = epochs.begin_poll();
        std::thread::scope(|threads| {
            threads
                .spawn(|| {
                    let overlap = epochs.begin_poll();
                    assert!(!epochs.owns_poll(overlap));
                    epochs.end_poll(overlap, true);
                })
                .join()
                .unwrap();
        });
        epochs.end_poll(first, true);
        assert_eq!(epochs.snapshot().generation, 0);

        let next = epochs.begin_poll();
        epochs.end_poll(next, true);
        assert_eq!(epochs.snapshot().generation, next.generation);
    }
}
