// SPDX-License-Identifier: MIT
//! F9 trigger admission and the Present-relative capture session. Native leaves poll Win32,
//! prepare the directory and graphics storage, change the tap, and lend artifacts to the writer.

use crate::motion_capture_observations::reset_session_observations;
use crate::motion_capture_records::reset_records;
use std::ffi::{c_char, c_void};
use std::mem::{offset_of, size_of};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub(super) const INTERVALS: u32 = 60;
const INACTIVE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CaptureInterval {
    pub(super) session: u32,
    pub(super) interval: u32,
}

impl CaptureInterval {
    const fn key(self) -> u64 {
        (self.session as u64) << 32 | self.interval as u64
    }

    const fn inactive(session: u32) -> Self {
        Self {
            session,
            interval: INACTIVE,
        }
    }

    pub(super) fn load() -> Self {
        let key = CURRENT.load(Ordering::Acquire);
        Self {
            session: (key >> 32) as u32,
            interval: key as u32,
        }
    }

    pub(super) fn active(self) -> bool {
        self.interval != INACTIVE
    }

    fn replace(self, next: Self) -> bool {
        CURRENT
            .compare_exchange(self.key(), next.key(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

// The session and interval are observed together so a detour cannot take one from each session.
static CURRENT: AtomicU64 = AtomicU64::new(CaptureInterval::inactive(0).key());
static ENABLED: AtomicBool = AtomicBool::new(false);
static REQUEST: AtomicBool = AtomicBool::new(false);
static FAILURES: AtomicU32 = AtomicU32::new(0);

struct PresentClock {
    presents: u64,
    start: u64,
    key_down: bool,
    callbacks_armed: bool,
}

static CLOCK: Mutex<PresentClock> = Mutex::new(PresentClock {
    presents: 0,
    start: 0,
    key_down: false,
    callbacks_armed: false,
});

// Private ABI kept with the native capture. Leaves are synchronous, do not throw, and do not
// reenter Present or shutdown. The Present lock serializes these effects and never gates hooks.
#[repr(C)]
struct CaptureEffects {
    poll: unsafe extern "C" fn(*mut c_void, *mut bool) -> bool,
    prepare: unsafe extern "C" fn(u32) -> bool,
    armed: unsafe extern "C" fn(CaptureInterval),
    set_sample: unsafe extern "C" fn(u32, *mut c_char, u32),
    finish: unsafe extern "C" fn(),
}

const _: () = {
    assert!(size_of::<CaptureInterval>() == 8 && offset_of!(CaptureInterval, interval) == 4);
    assert!(size_of::<CaptureEffects>() == 40 && offset_of!(CaptureEffects, finish) == 32);
};

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_enable(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_request() {
    REQUEST.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_interval() -> CaptureInterval {
    CaptureInterval::load()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_sampled(interval: u32) -> bool {
    matches!(interval, 0 | 30 | 59)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_sample() -> CaptureInterval {
    let observed = CaptureInterval::load();
    if rsf_ac7_capture_sampled(observed.interval) {
        observed
    } else {
        CaptureInterval::inactive(observed.session)
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_current(session: u32) -> bool {
    let observed = CaptureInterval::load();
    observed.active() && observed.session == session
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_fail(observed: CaptureInterval) {
    FAILURES.fetch_add(1, Ordering::Relaxed);
    if observed.active() {
        loop {
            let current = CaptureInterval::load();
            if !current.active()
                || current.session != observed.session
                || current.replace(CaptureInterval::inactive(current.session))
            {
                break;
            }
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_failures() -> u32 {
    FAILURES.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_due_checkpoint(observed: CaptureInterval) -> bool {
    rsf_ac7_capture_sampled(observed.interval) && rsf_ac7_capture_current(observed.session)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_present(
    swapchain: *mut c_void,
    prefix: *mut c_char,
    capacity: u32,
    effects: &CaptureEffects,
) -> bool {
    if !rsf_ac7_capture_enabled() {
        return false;
    }
    let mut clock = CLOCK.lock().unwrap_or_else(|error| error.into_inner());
    // SAFETY: The public native wrapper has checked the output and chain arguments.
    unsafe { prefix.write(0) };
    clock.presents = clock.presents.wrapping_add(1);

    let mut pressed = false;
    if !unsafe { (effects.poll)(swapchain, &mut pressed) } {
        // GetDesc failure still increments the clock, but does not consume a trigger or change
        // the key latch, callbacks or interval until a successful poll.
        return false;
    }
    let trigger = REQUEST.swap(false, Ordering::AcqRel) || (pressed && !clock.key_down);
    clock.key_down = pressed;
    let current = CaptureInterval::load();
    if trigger && !current.active() {
        if clock.callbacks_armed {
            unsafe { (effects.set_sample)(INACTIVE, prefix, capacity) };
            clock.callbacks_armed = false;
        }
        let session = current.session.wrapping_add(1);
        if unsafe { (effects.prepare)(session) } {
            reset_session_observations();
            reset_records();
            clock.start = clock.presents;
            let started = CaptureInterval {
                session,
                interval: 0,
            };
            CURRENT.store(started.key(), Ordering::Release);
            unsafe { (effects.armed)(started) };
        }
    }

    let current = CaptureInterval::load();
    if current.active() {
        let age = clock.presents.wrapping_sub(clock.start) as u32;
        if age >= INTERVALS {
            if current.replace(CaptureInterval::inactive(current.session)) {
                unsafe { (effects.set_sample)(INACTIVE, prefix, capacity) };
                clock.callbacks_armed = false;
                unsafe { (effects.finish)() };
                return false;
            }
        } else if current.replace(CaptureInterval {
            session: current.session,
            interval: age,
        }) {
            let sample = rsf_ac7_capture_sampled(age);
            unsafe { (effects.set_sample)(if sample { age } else { INACTIVE }, prefix, capacity) };
            clock.callbacks_armed = sample;
            if !sample || rsf_ac7_capture_current(current.session) {
                return sample;
            }
            // A failure raced callback registration. Suppress this prefix before returning.
            unsafe { prefix.write(0) };
        }
    }

    // An observation can fail on another thread. Its lease is already inactive. Retire
    // the tap here, serialized with a possible new session's callback registration.
    if clock.callbacks_armed {
        unsafe { (effects.set_sample)(INACTIVE, prefix, capacity) };
        clock.callbacks_armed = false;
    }
    false
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_shutdown(effects: &CaptureEffects) {
    let mut clock = CLOCK.lock().unwrap_or_else(|error| error.into_inner());
    let current = CaptureInterval::load();
    CURRENT.store(
        CaptureInterval::inactive(current.session).key(),
        Ordering::Release,
    );
    // SAFETY: A clear has no prefix to write.
    unsafe { (effects.set_sample)(INACTIVE, std::ptr::null_mut(), 0) };
    clock.callbacks_armed = false;
}
