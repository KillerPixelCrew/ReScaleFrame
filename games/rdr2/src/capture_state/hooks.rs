// SPDX-License-Identifier: MIT
//! Process-lived D3D12 capture detours. Native code calls each original with its typed ABI.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

// Order matches the private rsf_rdr2_capture_hook enum. Slot numbers are D3D12/DXGI vtable slots.
const SLOTS: [u32; 16] = [
    10, 12, 20, 44, 9, 47, 13, 10, 25, 46, 12, 13, 26, 27, 29, 40,
];
const HOOK_COUNT: usize = SLOTS.len();
const OK: i32 = 0;
const ALREADY_ENABLED: i32 = 5;
const UNKNOWN: i32 = -1;

#[repr(C)]
struct Ops {
    target: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
    create: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    enable: unsafe extern "C" fn(*mut c_void) -> i32,
    report: unsafe extern "C" fn(u32, *mut c_void, i32),
}

const _: () = assert!(size_of::<Ops>() == 32);

#[derive(Clone, Copy)]
enum Hook {
    Vacant,
    Installing,
    Created { target: usize, replacement: usize },
    Enabled,
    Refused,
}

static HOOKS: Mutex<[Hook; HOOK_COUNT]> = Mutex::new([Hook::Vacant; HOOK_COUNT]);
static ORIGINALS: [AtomicUsize; HOOK_COUNT] = [const { AtomicUsize::new(0) }; HOOK_COUNT];

fn hooks() -> MutexGuard<'static, [Hook; HOOK_COUNT]> {
    HOOKS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_hook_original(role: u32) -> usize {
    ORIGINALS
        .get(role as usize)
        .map_or(0, |slot| slot.load(Ordering::Acquire))
}

fn enable(ops: &Ops, role: usize, slot: u32, target: *mut c_void, replacement: *const c_void) {
    let status = unsafe { (ops.enable)(target) };
    hooks()[role] = if matches!(status, OK | ALREADY_ENABLED) {
        Hook::Enabled
    } else {
        Hook::Created {
            target: target.addr(),
            replacement: replacement.addr(),
        }
    };
    unsafe { (ops.report)(slot, target, status) };
}

/// The caller lends a live COM interface and a process-lived typed detour.
#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_bind_hook(
    ops: &Ops,
    role: u32,
    object: *mut c_void,
    replacement: *const c_void,
) {
    let role = role as usize;
    let Some(&slot) = SLOTS.get(role) else { return };
    if object.is_null() || replacement.is_null() {
        return;
    }
    let previous = {
        let mut hooks = hooks();
        match hooks[role] {
            previous @ (Hook::Vacant | Hook::Created { .. }) => {
                hooks[role] = Hook::Installing;
                previous
            }
            _ => return,
        }
    };
    // The native methods can reenter. Rust does not hold the registration lock across them.
    let target = unsafe { (ops.target)(object, slot) };
    if target.is_null() {
        hooks()[role] = previous;
        return;
    }
    if let Hook::Created {
        target: created,
        replacement: detour,
    } = previous
    {
        if created == target.addr() && detour == replacement.addr() {
            enable(ops, role, slot, target, replacement);
        } else {
            hooks()[role] = previous;
        }
        return;
    }
    let mut original = ptr::null_mut();
    let status = unsafe { (ops.create)(target, replacement, &mut original) };
    if status != OK || original.is_null() {
        hooks()[role] = if status == OK {
            Hook::Refused
        } else {
            Hook::Vacant
        };
        unsafe { (ops.report)(slot, target, if status == OK { UNKNOWN } else { status }) };
        return;
    }
    // A detour may be reached as soon as enable publishes the patch.
    ORIGINALS[role].store(original.addr(), Ordering::Release);
    enable(ops, role, slot, target, replacement);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32};

    static CREATES: AtomicU32 = AtomicU32::new(0);
    static ENABLES: AtomicU32 = AtomicU32::new(0);
    static PUBLISHED_BEFORE_ENABLE: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" fn target(object: *mut c_void, _slot: u32) -> *mut c_void {
        object
    }

    unsafe extern "C" fn create(
        target: *mut c_void,
        _replacement: *const c_void,
        original: *mut *mut c_void,
    ) -> i32 {
        CREATES.fetch_add(1, Ordering::Relaxed);
        unsafe { original.write(target) };
        OK
    }

    unsafe extern "C" fn enable(target: *mut c_void) -> i32 {
        PUBLISHED_BEFORE_ENABLE.store(
            rsf_rdr2_capture_hook_original(0) == target.addr(),
            Ordering::Relaxed,
        );
        if ENABLES.fetch_add(1, Ordering::Relaxed) == 0 {
            UNKNOWN
        } else {
            OK
        }
    }

    unsafe extern "C" fn report(_slot: u32, _target: *mut c_void, _status: i32) {}

    #[test]
    fn refused_enable_retries_only_the_created_target() {
        let ops = Ops {
            target,
            create,
            enable,
            report,
        };
        let (mut object, mut other, detour) = (0_u8, 1_u8, 2_u8);
        let object = ptr::from_mut(&mut object).cast();
        let other = ptr::from_mut(&mut other).cast();
        let detour = ptr::from_ref(&detour).cast();

        rsf_rdr2_capture_bind_hook(&ops, 0, object, detour);
        rsf_rdr2_capture_bind_hook(&ops, 0, other, detour);
        assert_eq!(ENABLES.load(Ordering::Relaxed), 1);
        rsf_rdr2_capture_bind_hook(&ops, 0, object, detour);
        rsf_rdr2_capture_bind_hook(&ops, 0, object, detour);

        assert_eq!(CREATES.load(Ordering::Relaxed), 1);
        assert_eq!(ENABLES.load(Ordering::Relaxed), 2);
        assert!(PUBLISHED_BEFORE_ENABLE.load(Ordering::Relaxed));
        hooks()[0] = Hook::Vacant;
        ORIGINALS[0].store(0, Ordering::Release);
    }
}
