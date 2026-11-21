// SPDX-License-Identifier: MIT
//! A late mode-1 poll may reserve a future source while the current tick is already bound.
//! Each transition holds the lock only while reading or updating those two scalar slots.

use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    None,
    Polling,
    Sampled,
    Adopted,
    Bound,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Reservation {
    source: u64,
    predecessor: u64,
    session: u64,
    job_generation: u64,
    poll_generation: u64,
    phase: Phase,
    produced: bool,
    kinds: u32,
}

impl Reservation {
    const EMPTY: Self = Self {
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
struct Reserve {
    reserved: Reservation,
    withdrawn: u64,
    overlap: bool,
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

// A null handle selects the process-lived store. A fixture owns a private allocation.
// Non-null handles must have come from new and remain live through every borrowed call.
unsafe fn lock<'a>(store: *const c_void) -> MutexGuard<'a, Sources> {
    unsafe { store.cast::<Store>().as_ref() }
        .unwrap_or(&PRODUCTION)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

const _: () = {
    assert!(size_of::<Reservation>() == 56);
    assert!(std::mem::offset_of!(Reservation, phase) == 40);
    assert!(std::mem::offset_of!(Reservation, kinds) == 48);
    assert!(size_of::<Reserve>() == 72);
};

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_sources_new() -> *mut c_void {
    Box::into_raw(Box::new(Mutex::new(Sources::EMPTY))).cast()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_free(store: *mut c_void) {
    if !store.is_null() {
        drop(unsafe { Box::from_raw(store.cast::<Store>()) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_reserve(
    store: *const c_void,
    out: &mut Reserve,
    source: u64,
    predecessor: u64,
    session: u64,
    job_generation: u64,
    poll_generation: u64,
) {
    *out = unsafe { lock(store) }.reserve(Reservation {
        source,
        predecessor,
        session,
        job_generation,
        poll_generation,
        phase: Phase::Polling,
        ..Reservation::EMPTY
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_still_reserved(store: *const c_void, source: u64) -> bool {
    unsafe { lock(store) }.pending.polling(source)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_sample(
    store: *const c_void,
    source: u64,
    produced: bool,
    kinds: u32,
) -> bool {
    unsafe { lock(store) }.sample(source, produced, kinds)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_withdraw_pending(store: *const c_void, only: u64) -> u64 {
    unsafe { lock(store) }.withdraw_pending(only)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_adopt(
    store: *const c_void,
    out: &mut Reservation,
    session: u64,
    joined: u64,
) {
    *out = unsafe { lock(store) }.adopt(session, joined);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_bind(
    store: *const c_void,
    source: u64,
    completed: u64,
) -> bool {
    unsafe { lock(store) }.bind(source, completed)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_owned(store: *const c_void, out: &mut Reservation) {
    *out = unsafe { lock(store) }.owned;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_release(
    store: *const c_void,
    out: &mut Reservation,
    source: u64,
) {
    *out = unsafe { lock(store) }.release(source);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sources_reset(store: *const c_void) {
    *unsafe { lock(store) } = Sources::EMPTY;
}

#[repr(u32)]
enum BindRefusal {
    None,
    Unreadable,
    QueuedMove,
    ConsumedBeforeBind,
    FreeCamera,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_bind_refusal(
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
