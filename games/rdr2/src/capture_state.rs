// SPDX-License-Identifier: MIT
//! Capture admission, limits, and publication. Native capture holds its existing mutex for
//! every ledger operation, ordering state with its graphics objects. The request and two fast
//! admission queries are atomic and can run without that mutex.
use std::alloc::{Layout, alloc};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(windows)]
mod artifacts;
#[cfg(windows)]
mod bundle;
mod gpu_leases;
mod recordings;
use gpu_leases::Gpu;
use recordings::Recordings;

const MAX_DRAW_COPIES: u32 = 64;
const MAX_DRAW_METADATA: u32 = 4096;
const INPUT_FRAMES: u32 = 4;
const CAPTURE_TIMEOUT_MS: u64 = 5000;
const SAVE_DRAW: u32 = 1;
const KEEP_TAIL: u32 = 2;

#[repr(C)]
struct Progress {
    sequence: u32,
    draws: u32,
    written: u32,
    draw_overflow: u32,
    timed_out: u32,
    write_failed: u32,
    reuse_tail: u32,
    requested_input_frames: u32,
}

const _: () = assert!(size_of::<Progress>() == 32);

#[derive(Default)]
struct Ledger {
    sequence: u32,
    draws: u32,
    written: u32,
    input_index: u32,
    inputs_in_flight: u32,
    started: u64,
    dirty: bool,
    draw_overflow: bool,
    timed_out: bool,
    write_failed: bool,
}

struct Capture {
    pending: AtomicBool,
    #[cfg(windows)]
    key_down: AtomicBool,
    active: AtomicBool,
    inputs_remaining: AtomicU32,
    ledger: UnsafeCell<Ledger>,
    recordings: UnsafeCell<Recordings>,
    gpu: UnsafeCell<Gpu>,
}

// The ASI passes its process-lived owner. The native mutex guards the non-atomic ledger.
unsafe impl Sync for Capture {}

impl Capture {
    fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            #[cfg(windows)]
            key_down: AtomicBool::new(false),
            active: AtomicBool::new(false),
            inputs_remaining: AtomicU32::new(0),
            ledger: UnsafeCell::new(Ledger::default()),
            recordings: UnsafeCell::new(Recordings::default()),
            gpu: UnsafeCell::new(Gpu::default()),
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_create() -> *mut Capture {
    // Return null on allocation refusal so the optional probe can decline installation.
    allocate(Capture::new())
}

fn allocate<T>(value: T) -> *mut T {
    let owner = unsafe { alloc(Layout::new::<T>()) }.cast::<T>();
    if !owner.is_null() {
        unsafe { owner.write(value) };
    }
    owner
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_request(owner: &Capture) {
    owner.pending.store(true, Ordering::Release);
}

/// The poll thread lends the NUL-terminated request filename for the duration of the call.
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_poll_request(owner: &Capture, request: *const u16) {
    use crate::diagnostics::{capture_key_down, foreground};
    use windows_sys::Win32::Storage::FileSystem::DeleteFileW;

    let down = capture_key_down();
    let was_down = owner.key_down.swap(down, Ordering::Relaxed);
    if down && !was_down && foreground() {
        rsf_rdr2_capture_request(owner);
    }
    if unsafe { DeleteFileW(request) } != 0 {
        rsf_rdr2_capture_request(owner);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_active(owner: &Capture) -> bool {
    owner.active.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_has_inputs(owner: &Capture) -> bool {
    owner.inputs_remaining.load(Ordering::Acquire) != 0
}

fn drained(owner: &Capture, ledger: &Ledger, readbacks_empty: bool) -> bool {
    readbacks_empty
        && !rsf_rdr2_capture_active(owner)
        && !rsf_rdr2_capture_has_inputs(owner)
        && ledger.inputs_in_flight == 0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_can_arm(owner: &Capture, readbacks_empty: bool) -> bool {
    let ledger = unsafe { &*owner.ledger.get() };
    owner.pending.load(Ordering::Acquire)
        && !ledger.dirty
        && drained(owner, ledger, readbacks_empty)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_arm(owner: &Capture, tick: u64) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    *ledger = Ledger {
        sequence: ledger.sequence.wrapping_add(1),
        started: tick,
        dirty: true,
        ..Ledger::default()
    };
    owner.pending.store(false, Ordering::Release);
    owner
        .inputs_remaining
        .store(INPUT_FRAMES, Ordering::Release);
    owner.active.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_end_draws(owner: &Capture) {
    owner.active.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_timeout(owner: &Capture, tick: u64) -> bool {
    let ledger = unsafe { &mut *owner.ledger.get() };
    if ledger.dirty
        && tick.wrapping_sub(ledger.started) > CAPTURE_TIMEOUT_MS
        && (rsf_rdr2_capture_active(owner) || rsf_rdr2_capture_has_inputs(owner))
    {
        rsf_rdr2_capture_end_draws(owner);
        owner.inputs_remaining.store(0, Ordering::Release);
        ledger.timed_out = true;
        return true;
    }
    false
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_progress_get(owner: &Capture, out: &mut Progress) {
    let ledger = unsafe { &*owner.ledger.get() };
    *out = Progress {
        sequence: ledger.sequence,
        draws: ledger.draws,
        written: ledger.written,
        draw_overflow: u32::from(ledger.draw_overflow),
        timed_out: u32::from(ledger.timed_out),
        write_failed: u32::from(ledger.write_failed),
        reuse_tail: u32::from(ledger.draws >= MAX_DRAW_COPIES),
        requested_input_frames: INPUT_FRAMES,
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_commit_draw(owner: &Capture, metadata_count: u32) -> u32 {
    let ledger = unsafe { &mut *owner.ledger.get() };
    let mut action = 0;
    if ledger.draws == MAX_DRAW_COPIES - 1 {
        action |= KEEP_TAIL;
    }
    if metadata_count < MAX_DRAW_METADATA {
        action |= SAVE_DRAW;
    } else {
        ledger.draw_overflow = true;
    }
    ledger.draws = ledger.draws.wrapping_add(1);
    action
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_reserve_input(owner: &Capture) -> u32 {
    let ledger = unsafe { &mut *owner.ledger.get() };
    let index = ledger.input_index;
    ledger.input_index = index.wrapping_add(1);
    index
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_begin_input(owner: &Capture) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    let remaining = owner.inputs_remaining.load(Ordering::Acquire);
    owner
        .inputs_remaining
        .store(remaining.saturating_sub(1), Ordering::Release);
    ledger.inputs_in_flight += 1;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_end_input(owner: &Capture) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    ledger.inputs_in_flight = ledger.inputs_in_flight.saturating_sub(1);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_readback_written(owner: &Capture, success: bool) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    if success {
        ledger.written = ledger.written.wrapping_add(1);
    } else {
        ledger.write_failed = true;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_write_failed(owner: &Capture) {
    unsafe { (*owner.ledger.get()).write_failed = true };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_can_publish(owner: &Capture, readbacks_empty: bool) -> bool {
    let ledger = unsafe { &*owner.ledger.get() };
    ledger.dirty && drained(owner, ledger, readbacks_empty)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_published(owner: &Capture) {
    unsafe { (*owner.ledger.get()).dirty = false };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_preserves_the_pending_request_and_waits_for_input_and_readbacks() {
        let owner = Capture::new();
        rsf_rdr2_capture_request(&owner);
        unsafe {
            assert!(rsf_rdr2_capture_can_arm(&owner, true));
            rsf_rdr2_capture_arm(&owner, 100);
            rsf_rdr2_capture_begin_input(&owner);
            rsf_rdr2_capture_request(&owner);
            assert!(!rsf_rdr2_capture_timeout(&owner, 5100));
            assert!(rsf_rdr2_capture_timeout(&owner, 5101));
            assert!(!rsf_rdr2_capture_can_publish(&owner, true));
            rsf_rdr2_capture_end_input(&owner);
            assert!(!rsf_rdr2_capture_can_publish(&owner, false));
            assert!(rsf_rdr2_capture_can_publish(&owner, true));
            assert!(!rsf_rdr2_capture_can_arm(&owner, true));
            rsf_rdr2_capture_published(&owner);
            assert!(rsf_rdr2_capture_can_arm(&owner, true));
        }
    }

    #[test]
    fn last_allocated_draw_is_retained_and_metadata_is_bounded() {
        let owner = Capture::new();
        unsafe {
            rsf_rdr2_capture_arm(&owner, 0);
            for count in 0..MAX_DRAW_COPIES - 1 {
                assert_eq!(rsf_rdr2_capture_commit_draw(&owner, count), SAVE_DRAW);
            }
            assert_eq!(
                rsf_rdr2_capture_commit_draw(&owner, MAX_DRAW_COPIES - 1),
                SAVE_DRAW | KEEP_TAIL
            );
            assert_eq!(rsf_rdr2_capture_commit_draw(&owner, MAX_DRAW_METADATA), 0);
        }
    }
}
