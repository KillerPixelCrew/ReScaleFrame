// SPDX-License-Identifier: MIT
//! Hook body guards of the native renderer.
//!
//! Every detour in native_renderer.cpp counts itself in and out so stop can tell when no thread
//! is inside the module: entry guards before hooks are disabled, entry and outer guards after.
//! Each thread keeps to one cache-line slot, so counting does not bounce a shared line between
//! the game, render and RHI threads; the slots are summed only on the stop and failure paths.
use crate::sites_abi::{RSF_AC7_GUARD_ENTRY, RSF_AC7_GUARD_OUTER};
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

fn counter(kind: u32) -> Option<&'static AtomicU32> {
    let index = INDEX.with(|index| {
        if index.get() == SLOTS {
            index.set(NEXT.fetch_add(1, Ordering::Relaxed) % SLOTS);
        }
        index.get()
    });
    let slot = &GUARDS[index as usize];
    match kind {
        RSF_AC7_GUARD_ENTRY => Some(&slot.entry),
        RSF_AC7_GUARD_OUTER => Some(&slot.outer),
        _ => None,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_guard_enter(kind: u32) {
    if let Some(count) = counter(kind) {
        count.fetch_add(1, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_guard_leave(kind: u32) {
    if let Some(count) = counter(kind) {
        count.fetch_sub(1, Ordering::Release);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_guards_held(outer_too: bool) -> u32 {
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
        assert_eq!(rsf_ac7_guards_held(true), 0);
        rsf_ac7_guard_enter(RSF_AC7_GUARD_ENTRY);
        rsf_ac7_guard_enter(RSF_AC7_GUARD_OUTER);
        assert_eq!(rsf_ac7_guards_held(false), 1);
        assert_eq!(rsf_ac7_guards_held(true), 2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                rsf_ac7_guard_enter(RSF_AC7_GUARD_ENTRY);
                assert_eq!(rsf_ac7_guards_held(false), 2);
                rsf_ac7_guard_leave(RSF_AC7_GUARD_ENTRY);
            });
        });
        // More threads than slots share slots without losing a count.
        std::thread::scope(|scope| {
            for _ in 0..SLOTS + 8 {
                scope.spawn(|| {
                    rsf_ac7_guard_enter(RSF_AC7_GUARD_OUTER);
                    rsf_ac7_guard_leave(RSF_AC7_GUARD_OUTER);
                });
            }
        });
        assert_eq!(rsf_ac7_guards_held(true), 2);
        rsf_ac7_guard_leave(RSF_AC7_GUARD_OUTER);
        rsf_ac7_guard_leave(RSF_AC7_GUARD_ENTRY);
        assert_eq!(rsf_ac7_guards_held(true), 0);
        rsf_ac7_guard_enter(7);
        assert_eq!(rsf_ac7_guards_held(true), 0);
        rsf_ac7_guard_leave(7);
        assert_eq!(rsf_ac7_guards_held(true), 0);
    }
}
