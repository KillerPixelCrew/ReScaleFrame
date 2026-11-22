// SPDX-License-Identifier: MIT
//! F9 trigger admission, output paths and the Present-relative capture session.

use crate::motion_capture_observations::reset_session_observations;
use crate::motion_capture_records::artifacts::{Bytes, Stats, WriteFile, publish_finish};
use crate::motion_capture_records::reset_records;
use std::ffi::{CStr, c_char, c_void};
use std::fmt::{self, Write as _};
use std::mem::{offset_of, size_of};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

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

type Log = unsafe extern "C" fn(*mut c_void, *const c_char);

struct Output {
    directory: Vec<u8>,
    prefix: Option<(u32, Arc<CStr>)>,
    log: Option<Log>,
    user: usize,
}

static OUTPUT: Mutex<Output> = Mutex::new(Output {
    directory: Vec::new(),
    prefix: None,
    log: None,
    user: 0,
});

fn output() -> MutexGuard<'static, Output> {
    OUTPUT.lock().unwrap_or_else(|error| error.into_inner())
}

pub(super) fn capture_prefix(session: u32) -> Option<Arc<CStr>> {
    output()
        .prefix
        .as_ref()
        .filter(|(owner, _)| *owner == session)
        .map(|(_, prefix)| Arc::clone(prefix))
}

pub(super) fn say(message: &CStr) {
    let (log, user) = {
        let output = output();
        (output.log, output.user)
    };
    if let Some(log) = log {
        // SAFETY: Configuration supplies a callback and user pointer valid for capture's lifetime.
        unsafe {
            log(
                std::ptr::with_exposed_provenance_mut(user),
                message.as_ptr(),
            )
        };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_log(message: *const c_char) {
    // SAFETY: Native supplies a static terminated diagnostic string.
    say(unsafe { CStr::from_ptr(message) });
}

pub(super) fn say_path(message: &str, prefix: &CStr, suffix: &str) -> fmt::Result {
    let mut text = Bytes::default();
    text.write_str(message)?;
    text.extend(prefix.to_bytes())?;
    text.write_str(suffix)?;
    text.write_char('\0')?;
    say(CStr::from_bytes_with_nul(&text.0).map_err(|_| fmt::Error)?);
    Ok(())
}

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
    stamp: unsafe extern "C" fn(*mut CaptureStamp),
    create_directory: unsafe extern "C" fn(*const c_char) -> bool,
    set_sample: unsafe extern "C" fn(bool),
    write: WriteFile,
    stats: unsafe extern "C" fn() -> Stats,
}

#[repr(C)]
#[derive(Default)]
struct CaptureStamp {
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    process: u32,
}

const _: () = {
    assert!(size_of::<CaptureInterval>() == 8 && offset_of!(CaptureInterval, interval) == 4);
    assert!(size_of::<CaptureStamp>() == 28 && offset_of!(CaptureStamp, process) == 24);
    assert!(size_of::<CaptureEffects>() == 48 && offset_of!(CaptureEffects, stats) == 40);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_motion_capture_configure(
    directory: *const c_char,
    log: Option<Log>,
    user: *mut c_void,
) {
    // SAFETY: The public ABI accepts a null or terminated narrow directory during this call.
    let directory = if directory.is_null() {
        c""
    } else {
        unsafe { CStr::from_ptr(directory) }
    };
    let mut bytes = Bytes::default();
    if bytes.extend(directory.to_bytes()).is_err() {
        rsf_ac7_capture_fail(CaptureInterval::load());
        return;
    }
    let enabled = !bytes.0.is_empty();
    {
        let mut output = output();
        output.directory = bytes.0;
        output.log = log;
        output.user = user.expose_provenance();
    }
    ENABLED.store(enabled, Ordering::Release);
    if enabled {
        say(c"motion capture configured: F9 records 60 intervals, sampled at 0/30/59");
    }
}

fn prepare_output(session: u32, effects: &CaptureEffects) -> Result<bool, fmt::Error> {
    let mut stamp = CaptureStamp::default();
    // SAFETY: Native supplies Win32 time and process ID in our writable stamp.
    unsafe { (effects.stamp)(&mut stamp) };
    let CaptureStamp {
        year,
        month,
        day,
        hour,
        minute,
        second,
        process,
    } = stamp;
    let mut path = Bytes::default();
    path.extend(&output().directory)?;
    write!(
        path,
        "\\motion-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{process}-{session}\0"
    )?;
    let path = CStr::from_bytes_with_nul(&path.0).map_err(|_| fmt::Error)?;
    // SAFETY: The native directory leaf borrows the original narrow path only for the call.
    if !unsafe { (effects.create_directory)(path.as_ptr()) } {
        say(c"motion capture: output directory could not be created");
        return Ok(false);
    }
    output().prefix = Some((session, Arc::from(path)));
    Ok(true)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_motion_capture_request() {
    REQUEST.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_interval() -> CaptureInterval {
    CaptureInterval::load()
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_sampled(interval: u32) -> bool {
    matches!(interval, 0 | 30 | 59)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_sample() -> CaptureInterval {
    let observed = CaptureInterval::load();
    if rsf_ac7_capture_sampled(observed.interval) {
        observed
    } else {
        CaptureInterval::inactive(observed.session)
    }
}

pub(super) fn rsf_ac7_capture_current(session: u32) -> bool {
    let observed = CaptureInterval::load();
    observed.active() && observed.session == session
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_fail(observed: CaptureInterval) {
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
    say(c"motion capture stopped after an allocation or serialization failure; game decisions are unchanged");
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_ac7_capture_failures() -> u32 {
    FAILURES.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_present(
    swapchain: *mut c_void,
    prefix: *mut c_char,
    capacity: u32,
    effects: &CaptureEffects,
) -> bool {
    if swapchain.is_null() || prefix.is_null() || capacity == 0 || !rsf_ac7_capture_enabled() {
        return false;
    }
    let mut clock = CLOCK.lock().unwrap_or_else(|error| error.into_inner());
    // SAFETY: The caller lends capacity writable bytes for the returned narrow path.
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
            unsafe { (effects.set_sample)(false) };
            clock.callbacks_armed = false;
        }
        let session = current.session.wrapping_add(1);
        let prepared = prepare_output(session, effects).unwrap_or_else(|_| {
            rsf_ac7_capture_fail(current);
            false
        });
        if prepared {
            reset_session_observations();
            reset_records();
            clock.start = clock.presents;
            let started = CaptureInterval {
                session,
                interval: 0,
            };
            CURRENT.store(started.key(), Ordering::Release);
            if let Some(path) = capture_prefix(session)
                && say_path("motion capture armed: ", &path, "").is_err()
            {
                rsf_ac7_capture_fail(started);
            }
        }
    }

    let current = CaptureInterval::load();
    if current.active() {
        let age = clock.presents.wrapping_sub(clock.start) as u32;
        if age >= INTERVALS {
            if current.replace(CaptureInterval::inactive(current.session)) {
                unsafe { (effects.set_sample)(false) };
                clock.callbacks_armed = false;
                publish_finish(current, effects.write, effects.stats);
                return false;
            }
        } else if current.replace(CaptureInterval {
            session: current.session,
            interval: age,
        }) {
            let sample = rsf_ac7_capture_sampled(age);
            unsafe { (effects.set_sample)(sample) };
            clock.callbacks_armed = sample;
            if !sample {
                return false;
            }
            if let Some(path) = capture_prefix(current.session) {
                let suffix = [
                    b'\\',
                    b'f',
                    b'0',
                    b'0' + (age / 10) as u8,
                    b'0' + (age % 10) as u8,
                ];
                let path = path.to_bytes();
                let path_len = path.len().min(capacity as usize - 1);
                let suffix_len = suffix.len().min(capacity as usize - 1 - path_len);
                // SAFETY: Both slices are independent of the caller buffer and the copy is bounded.
                unsafe {
                    std::ptr::copy_nonoverlapping(path.as_ptr(), prefix.cast(), path_len);
                    std::ptr::copy_nonoverlapping(
                        suffix.as_ptr(),
                        prefix.add(path_len).cast(),
                        suffix_len,
                    );
                    prefix.add(path_len + suffix_len).write(0);
                }
                if rsf_ac7_capture_current(current.session) {
                    return true;
                }
            }
            // A failure raced callback registration. Suppress this prefix before returning.
            unsafe { prefix.write(0) };
        }
    }

    // An observation can fail on another thread. Its lease is already inactive. Retire
    // the tap here, serialized with a possible new session's callback registration.
    if clock.callbacks_armed {
        unsafe { (effects.set_sample)(false) };
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
    // SAFETY: This only clears the tap callbacks.
    unsafe { (effects.set_sample)(false) };
    clock.callbacks_armed = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_PATH: &CStr = c"capture\xff\\motion-20261122-101519-42-1";
    static WRITES: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    static TAP_ARMED: AtomicBool = AtomicBool::new(false);

    #[test]
    fn present_returns_the_original_narrow_path_at_each_sample() {
        let effects = CaptureEffects {
            poll,
            stamp,
            create_directory,
            set_sample,
            write,
            stats: empty_stats,
        };
        // SAFETY: The directory is static and the fixture has no log callback.
        unsafe {
            rsf_ac7_motion_capture_configure(c"capture\xff".as_ptr(), None, std::ptr::null_mut())
        };
        rsf_ac7_motion_capture_request();
        for interval in 0..=INTERVALS {
            let mut prefix = [0 as c_char; 128];
            // SAFETY: The fake poll does not dereference its non-null swap chain identity, and the
            // output lends the capacity passed to Present.
            let sampled = unsafe {
                rsf_ac7_capture_present(
                    std::ptr::dangling_mut::<c_void>(),
                    prefix.as_mut_ptr(),
                    prefix.len() as u32,
                    &effects,
                )
            };
            assert_eq!(sampled, matches!(interval, 0 | 30 | 59));
            assert_eq!(TAP_ARMED.load(Ordering::Relaxed), sampled);
            // SAFETY: The stack buffer is zeroed before the call and Present preserves a terminator.
            let path = unsafe { CStr::from_ptr(prefix.as_ptr()) };
            if sampled {
                assert_eq!(
                    path.to_bytes(),
                    [
                        SESSION_PATH.to_bytes(),
                        format!("\\f{interval:03}").as_bytes()
                    ]
                    .concat()
                );
            } else {
                assert!(path.to_bytes().is_empty());
            }
        }
        assert_eq!(
            WRITES.lock().unwrap().last().unwrap(),
            &[SESSION_PATH.to_bytes(), b"\\session.json"].concat()
        );
        // SAFETY: The fixture supplies the same live effects table for shutdown.
        unsafe { rsf_ac7_capture_shutdown(&effects) };
    }

    unsafe extern "C" fn poll(_: *mut c_void, pressed: *mut bool) -> bool {
        // SAFETY: Present lends its writable key state.
        unsafe { pressed.write(false) };
        true
    }

    unsafe extern "C" fn stamp(output: *mut CaptureStamp) {
        // SAFETY: Present lends its writable Win32 stamp.
        unsafe {
            output.write(CaptureStamp {
                year: 2026,
                month: 11,
                day: 22,
                hour: 10,
                minute: 15,
                second: 19,
                process: 42,
            })
        };
    }

    unsafe extern "C" fn create_directory(path: *const c_char) -> bool {
        // SAFETY: Present lends its terminated path.
        unsafe { CStr::from_ptr(path) == SESSION_PATH }
    }

    extern "C" fn set_sample(enabled: bool) {
        TAP_ARMED.store(enabled, Ordering::Relaxed);
    }

    unsafe extern "C" fn write(path: *const c_char, _: *const u8, _: usize) -> bool {
        // SAFETY: The publication lends its terminated filename until this call returns.
        let path = unsafe { CStr::from_ptr(path) };
        WRITES.lock().unwrap().push(path.to_bytes().to_vec());
        true
    }

    extern "C" fn empty_stats() -> Stats {
        Stats::default()
    }
}
