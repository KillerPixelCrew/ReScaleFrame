// SPDX-License-Identifier: MIT
//! Capture admission, limits, and publication. The observation mutex guards
//! ledger operations and orders them with the graphics objects. Request and fast admission
//! queries are atomic. The NGX wrapper nesting is tracked on the calling thread.
// Unit tests compile the owner without the native detours that call most of it.
#![cfg_attr(test, allow(dead_code))]
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(windows)]
mod artifacts;
#[cfg(windows)]
mod bundle;
#[cfg(windows)]
mod d3d12;
#[cfg(all(windows, not(test)))]
mod detours;
#[cfg(all(windows, feature = "capture-fixture", not(test)))]
mod fixture;
mod gpu_leases;
#[cfg(any(windows, test))]
mod hooks;
#[cfg(all(windows, not(test)))]
mod install;
mod metadata;
#[cfg(all(windows, not(test)))]
mod ngx;
#[cfg(windows)]
mod pipeline;
#[cfg(all(windows, not(test)))]
mod poll;
mod recordings;
mod shots;
use gpu_leases::Gpu;
use metadata::{Metadata, PresentationDesc};
use recordings::Recordings;
use shots::{Ready, Shots};

const MAX_DRAW_COPIES: u32 = 64;
const MAX_DRAW_METADATA: usize = 4096;
const INPUT_FRAMES: u32 = 4;
#[cfg(any(windows, test))]
const CAPTURE_TIMEOUT_MS: u64 = 5000;
const SAVE_DRAW: u32 = 1;
const KEEP_TAIL: u32 = 2;

#[cfg(windows)]
struct Progress {
    sequence: u32,
    draws: u32,
    written: u32,
    draw_overflow: u32,
    timed_out: u32,
    write_failed: u32,
    requested_input_frames: u32,
    readbacks_unavailable: u32,
}

#[derive(Default)]
#[cfg_attr(not(windows), allow(dead_code))]
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
    readbacks_unavailable: u32,
}

#[cfg(windows)]
impl Ledger {
    fn progress(&self) -> Progress {
        Progress {
            sequence: self.sequence,
            draws: self.draws,
            written: self.written,
            draw_overflow: u32::from(self.draw_overflow),
            timed_out: u32::from(self.timed_out),
            write_failed: u32::from(self.write_failed),
            requested_input_frames: INPUT_FRAMES,
            readbacks_unavailable: self.readbacks_unavailable,
        }
    }
}

struct Capture {
    pending: AtomicBool,
    #[cfg(all(windows, not(test)))]
    key_down: AtomicBool,
    active: AtomicBool,
    inputs_remaining: AtomicU32,
    ledger: UnsafeCell<Ledger>,
    metadata: UnsafeCell<Metadata>,
    recordings: UnsafeCell<Recordings>,
    gpu: UnsafeCell<Gpu>,
    shots: UnsafeCell<Shots>,
    // The poll thread alone accesses this batch. It detaches shots under the native mutex.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    ready: UnsafeCell<Ready>,
}

// The ASI keeps the owner for the life of the process. The native mutex guards shared
// non-atomic state. The detached ready batch is exclusive to the poll thread.
unsafe impl Sync for Capture {}

#[cfg(any(windows, test))]
impl Capture {
    fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            #[cfg(all(windows, not(test)))]
            key_down: AtomicBool::new(false),
            active: AtomicBool::new(false),
            inputs_remaining: AtomicU32::new(0),
            ledger: UnsafeCell::new(Ledger::default()),
            metadata: UnsafeCell::new(Metadata::default()),
            recordings: UnsafeCell::new(Recordings::default()),
            gpu: UnsafeCell::new(Gpu::default()),
            shots: UnsafeCell::new(Shots::default()),
            ready: UnsafeCell::new(Ready::default()),
        }
    }
}

fn try_entry<V: Default>(map: &mut HashMap<usize, V>, key: usize) -> Option<&mut V> {
    if !map.contains_key(&key) && map.try_reserve(1).is_err() {
        return None;
    }
    Some(map.entry(key).or_default())
}

fn request(owner: &Capture) {
    owner.pending.store(true, Ordering::Release);
}

/// The poll thread lends the NUL-terminated request filename for the duration of the call.
#[cfg(all(windows, not(test)))]
unsafe fn poll_request(owner: &Capture, request_file: *const u16) {
    use crate::diagnostics::{capture_key_down, capture_key_pressed, foreground};
    use windows_sys::Win32::Storage::FileSystem::DeleteFileW;

    let down = capture_key_down();
    let was_down = owner.key_down.swap(down, Ordering::Relaxed);
    if capture_key_pressed(was_down, down, foreground()) {
        request(owner);
    }
    if unsafe { DeleteFileW(request_file) } != 0 {
        request(owner);
    }
}

fn active(owner: &Capture) -> bool {
    owner.active.load(Ordering::Acquire)
}

fn has_inputs(owner: &Capture) -> bool {
    owner.inputs_remaining.load(Ordering::Acquire) != 0
}

unsafe fn drained(owner: &Capture, ledger: &Ledger) -> bool {
    unsafe { &*owner.shots.get() }.pending.is_empty()
        && !active(owner)
        && !has_inputs(owner)
        && ledger.inputs_in_flight == 0
}

unsafe fn can_arm(owner: &Capture) -> bool {
    let ledger = unsafe { &*owner.ledger.get() };
    owner.pending.load(Ordering::Acquire) && !ledger.dirty && unsafe { drained(owner, ledger) }
}

unsafe fn arm(
    owner: &Capture,
    tick: u64,
    swapchain_identity: *mut c_void,
    descriptor: &PresentationDesc,
) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    *ledger = Ledger {
        sequence: ledger.sequence.wrapping_add(1),
        started: tick,
        dirty: true,
        ..Ledger::default()
    };
    unsafe { &mut *owner.metadata.get() }.arm(swapchain_identity.addr(), *descriptor);
    unsafe { &mut *owner.recordings.get() }.draws.clear();
    owner.pending.store(false, Ordering::Release);
    owner
        .inputs_remaining
        .store(INPUT_FRAMES, Ordering::Release);
    owner.active.store(true, Ordering::Release);
}

fn end_draws(owner: &Capture) {
    owner.active.store(false, Ordering::Release);
}

#[cfg(any(windows, test))]
unsafe fn timeout(owner: &Capture, tick: u64) -> bool {
    let ledger = unsafe { &mut *owner.ledger.get() };
    if ledger.dirty
        && tick.wrapping_sub(ledger.started) > CAPTURE_TIMEOUT_MS
        && (active(owner) || has_inputs(owner))
    {
        end_draws(owner);
        owner.inputs_remaining.store(0, Ordering::Release);
        ledger.timed_out = true;
        return true;
    }
    false
}

unsafe fn commit_draw(owner: &Capture) -> u32 {
    let ledger = unsafe { &mut *owner.ledger.get() };
    let draw_count = unsafe { &*owner.recordings.get() }.draws.len();
    let mut action = 0;
    if ledger.draws == MAX_DRAW_COPIES - 1 {
        action |= KEEP_TAIL;
    }
    if draw_count < MAX_DRAW_METADATA {
        action |= SAVE_DRAW;
    } else {
        ledger.draw_overflow = true;
    }
    ledger.draws = ledger.draws.wrapping_add(1);
    action
}

#[cfg(all(windows, not(test)))]
unsafe fn readback_written(owner: &Capture, success: bool) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    if success {
        ledger.written = ledger.written.wrapping_add(1);
    } else {
        ledger.write_failed = true;
    }
}

#[cfg(any(windows, test))]
unsafe fn can_publish(owner: &Capture) -> bool {
    let ledger = unsafe { &*owner.ledger.get() };
    ledger.dirty && unsafe { drained(owner, ledger) }
}

#[cfg(any(windows, test))]
unsafe fn mark_published(owner: &Capture, sequence: u32) {
    let ledger = unsafe { &mut *owner.ledger.get() };
    if ledger.sequence == sequence {
        ledger.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::{null_mut, without_provenance_mut};

    #[test]
    fn timeout_preserves_the_pending_request_and_waits_for_input_and_readbacks() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let feature = without_provenance_mut(2);
        request(&owner);
        unsafe {
            assert!(can_arm(&owner));
            arm(&owner, 100, null_mut(), &PresentationDesc::default());
            metadata::record_feature(&owner, feature, Some(0));
            assert!(metadata::track_evaluation(false));
            assert!(metadata::begin_input(&owner, list, feature).is_some());
            assert!(metadata::commit_input(&owner));
            assert!(!metadata::track_evaluation(false));
            assert!(!metadata::track_evaluation(true));
            request(&owner);
            assert!(!timeout(&owner, 5100));
            assert!(timeout(&owner, 5101));
            assert!(!can_publish(&owner));
            assert!(metadata::track_evaluation(true));
            assert!(!metadata::track_evaluation(false));
            assert!(!metadata::track_evaluation(true));
            metadata::finish_input(&owner);
            (&mut *owner.shots.get())
                .pending
                .push(shots::Shot::default());
            assert!(!can_publish(&owner));
            (&mut *owner.shots.get()).pending.clear();
            assert!(can_publish(&owner));
            assert!(!can_arm(&owner));
            mark_published(&owner, 1);
            assert!(can_arm(&owner));
        }
    }

    #[test]
    fn last_allocated_draw_is_retained_and_metadata_is_bounded() {
        let owner = Capture::new();
        unsafe {
            arm(&owner, 0, null_mut(), &PresentationDesc::default());
            for _ in 0..MAX_DRAW_COPIES - 1 {
                assert_eq!(commit_draw(&owner), SAVE_DRAW);
            }
            assert_eq!(commit_draw(&owner), SAVE_DRAW | KEEP_TAIL);
            (&mut *owner.recordings.get())
                .draws
                .resize(MAX_DRAW_METADATA, recordings::Draw::default());
            assert_eq!(commit_draw(&owner), 0);
        }
    }
}
