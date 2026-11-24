// SPDX-License-Identifier: MIT
//! Process-lived D3D12 capture detours. Detours call each original with its typed ABI.

#[cfg(not(test))]
use super::detours::{create_hook, enable_hook, hook_report, vtable_target};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(test)]
use tests::{create_hook, enable_hook, hook_report, vtable_target};

// Order matches detours::Role and the persisted hook-role mask.
pub(super) const SLOTS: [u32; 16] = [
    10, 12, 20, 44, 9, 47, 13, 10, 25, 46, 12, 13, 26, 27, 29, 40,
];
const HOOK_COUNT: usize = SLOTS.len();
const OK: i32 = 0;
const ALREADY_ENABLED: i32 = 5;
const UNKNOWN: i32 = -1;
pub(super) const DIFFERENT_TARGET: i32 = -2;

#[derive(Clone, Copy)]
enum Hook {
    Vacant,
    Installing { target: usize },
    Created { target: usize, replacement: usize },
    Enabled { target: usize },
    Refused,
}

static HOOKS: Mutex<[Hook; HOOK_COUNT]> = Mutex::new([Hook::Vacant; HOOK_COUNT]);
static ORIGINALS: [AtomicPtr<c_void>; HOOK_COUNT] =
    [const { AtomicPtr::new(ptr::null_mut()) }; HOOK_COUNT];
static UNOBSERVED_HOOK_ROLES: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
pub(super) fn unobserved_roles() -> u32 {
    UNOBSERVED_HOOK_ROLES.load(Ordering::Acquire)
}

fn different_target(role: usize, slot: u32, target: *mut c_void) {
    let bit = 1 << role;
    if UNOBSERVED_HOOK_ROLES.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
        unsafe { hook_report(slot, target, DIFFERENT_TARGET) };
    }
}

fn hooks() -> MutexGuard<'static, [Hook; HOOK_COUNT]> {
    HOOKS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(any(all(windows, not(test)), test))]
pub(super) fn original(role: u32) -> *mut c_void {
    ORIGINALS
        .get(role as usize)
        .map_or(ptr::null_mut(), |slot| slot.load(Ordering::Acquire))
}

fn enable(role: usize, slot: u32, target: *mut c_void, replacement: *const c_void) {
    let status = unsafe { enable_hook(target) };
    hooks()[role] = if matches!(status, OK | ALREADY_ENABLED) {
        Hook::Enabled {
            target: target.addr(),
        }
    } else {
        Hook::Created {
            target: target.addr(),
            replacement: replacement.addr(),
        }
    };
    unsafe { hook_report(slot, target, status) };
}

/// The caller lends a live COM interface and a process-lived typed detour.
pub(super) fn bind(role: u32, object: *mut c_void, replacement: *const c_void) {
    let role = role as usize;
    let Some(&slot) = SLOTS.get(role) else { return };
    if object.is_null() || replacement.is_null() {
        return;
    }
    // The target lookup can reenter. Keep it outside the registry lock.
    let target = unsafe { vtable_target(object, slot) };
    if target.is_null() {
        return;
    }
    let previous = {
        let mut hooks = hooks();
        match hooks[role] {
            previous @ (Hook::Vacant | Hook::Created { .. }) => {
                hooks[role] = Hook::Installing {
                    target: target.addr(),
                };
                previous
            }
            Hook::Enabled { target: installed } | Hook::Installing { target: installed }
                if installed != target.addr() =>
            {
                drop(hooks);
                different_target(role, slot, target);
                return;
            }
            _ => return,
        }
    };
    if let Hook::Created {
        target: created,
        replacement: detour,
    } = previous
    {
        if created == target.addr() && detour == replacement.addr() {
            enable(role, slot, target, replacement);
        } else {
            hooks()[role] = previous;
            different_target(role, slot, target);
        }
        return;
    }
    let mut original = ptr::null_mut();
    let status = unsafe { create_hook(target, replacement, &mut original) };
    if status != OK || original.is_null() {
        hooks()[role] = if status == OK {
            Hook::Refused
        } else {
            Hook::Vacant
        };
        unsafe { hook_report(slot, target, if status == OK { UNKNOWN } else { status }) };
        return;
    }
    // A detour may be reached as soon as enable publishes the patch.
    ORIGINALS[role].store(original, Ordering::Release);
    enable(role, slot, target, replacement);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32};

    static CREATES: AtomicU32 = AtomicU32::new(0);
    static ENABLES: AtomicU32 = AtomicU32::new(0);
    static PUBLISHED_BEFORE_ENABLE: AtomicBool = AtomicBool::new(false);

    pub(super) unsafe fn vtable_target(object: *mut c_void, _slot: u32) -> *mut c_void {
        object
    }

    pub(super) unsafe fn create_hook(
        target: *mut c_void,
        _replacement: *const c_void,
        original: *mut *mut c_void,
    ) -> i32 {
        CREATES.fetch_add(1, Ordering::Relaxed);
        unsafe { original.write(target) };
        OK
    }

    pub(super) unsafe fn enable_hook(target: *mut c_void) -> i32 {
        PUBLISHED_BEFORE_ENABLE.store(original(0) == target, Ordering::Relaxed);
        if ENABLES.fetch_add(1, Ordering::Relaxed) == 0 {
            UNKNOWN
        } else {
            OK
        }
    }

    pub(super) unsafe fn hook_report(_slot: u32, _target: *mut c_void, _status: i32) {}

    #[test]
    fn refused_enable_retries_only_the_created_target() {
        let (mut object, mut other, detour) = (0_u8, 1_u8, 2_u8);
        let object = ptr::from_mut(&mut object).cast();
        let other = ptr::from_mut(&mut other).cast();
        let detour = ptr::from_ref(&detour).cast();

        bind(0, object, detour);
        bind(0, other, detour);
        assert_eq!(ENABLES.load(Ordering::Relaxed), 1);
        assert_eq!(UNOBSERVED_HOOK_ROLES.load(Ordering::Acquire) & 1, 1);
        UNOBSERVED_HOOK_ROLES.store(0, Ordering::Release);
        bind(0, object, detour);
        bind(0, object, detour);
        bind(0, other, detour);

        assert_eq!(CREATES.load(Ordering::Relaxed), 1);
        assert_eq!(ENABLES.load(Ordering::Relaxed), 2);
        assert!(PUBLISHED_BEFORE_ENABLE.load(Ordering::Relaxed));
        assert_eq!(UNOBSERVED_HOOK_ROLES.load(Ordering::Acquire) & 1, 1);
        hooks()[0] = Hook::Vacant;
        ORIGINALS[0].store(ptr::null_mut(), Ordering::Release);
        UNOBSERVED_HOOK_ROLES.store(0, Ordering::Release);
    }
}
