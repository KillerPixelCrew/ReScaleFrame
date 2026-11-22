// SPDX-License-Identifier: MIT
//! Hook body guards of the native renderer.
//!
//! Every detour in native_renderer.rs counts itself in and out so stop can tell when no thread
//! is inside the module: entry guards before hooks are disabled, entry and outer guards after.
//! Each thread keeps to one cache-line slot, so counting does not bounce a shared line between
//! the game, render and RHI threads; the slots are summed only on the stop and failure paths.
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering, fence};

const SLOTS: u32 = 64;

#[repr(align(64))]
struct Slot {
    entry: AtomicU32,
    outer: AtomicU32,
}

static GUARDS: [Slot; SLOTS as usize] = [const {
    Slot {
        entry: AtomicU32::new(0),
        outer: AtomicU32::new(0),
    }
}; SLOTS as usize];
static NEXT: AtomicU32 = AtomicU32::new(0);

thread_local! {
    // SLOTS until the thread's first guard picks a slot. Nothing to drop, so hooks running
    // during thread teardown still reach it.
    static INDEX: Cell<u32> = const { Cell::new(SLOTS) };
}

/// This thread's entry or outer count.
fn counter(outer: bool) -> &'static AtomicU32 {
    let index = INDEX.with(|index| {
        if index.get() == SLOTS {
            index.set(NEXT.fetch_add(1, Ordering::Relaxed) % SLOTS);
        }
        index.get()
    });
    let slot = &GUARDS[index as usize];
    if outer { &slot.outer } else { &slot.entry }
}

pub(crate) fn enter(outer: bool) {
    counter(outer).fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn leave(outer: bool) {
    counter(outer).fetch_sub(1, Ordering::Release);
}

/// Hook bodies running: entry guards, plus outer guards when `outer_too`.
pub(crate) fn held(outer_too: bool) -> u32 {
    fence(Ordering::SeqCst);
    GUARDS.iter().fold(0u32, |total, slot| {
        let total = total.wrapping_add(slot.entry.load(Ordering::Acquire));
        if outer_too {
            total.wrapping_add(slot.outer.load(Ordering::Acquire))
        } else {
            total
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the slots are process-wide, so parallel tests would see each other's guards.
    #[test]
    fn guards_count_per_thread_and_sum_by_kind() {
        assert_eq!(held(true), 0);
        enter(false);
        enter(true);
        assert_eq!(held(false), 1);
        assert_eq!(held(true), 2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                enter(false);
                assert_eq!(held(false), 2);
                leave(false);
            });
        });
        // More threads than slots share slots without losing a count.
        std::thread::scope(|scope| {
            for _ in 0..SLOTS + 8 {
                scope.spawn(|| {
                    enter(true);
                    leave(true);
                });
            }
        });
        assert_eq!(held(true), 2);
        leave(true);
        leave(false);
        assert_eq!(held(true), 0);
    }
}
