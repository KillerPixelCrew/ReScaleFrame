// SPDX-License-Identifier: MIT
//! Capture hook batch ownership. Native leaves perform the MinHook calls.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::slice;

type Target = *mut c_void;
type Create = unsafe extern "C" fn(Target, Target, *mut Target) -> bool;
type Hook = unsafe extern "C" fn(Target) -> bool;

fn remove(targets: &mut [Target], disable: Hook, retire: Hook) {
    for target in targets.iter_mut().filter(|target| !target.is_null()) {
        unsafe { disable(*target) };
        if unsafe { retire(*target) } {
            *target = null_mut();
        }
    }
}

/// The arrays contain `count` entries for the duration of the call. A null target is skipped.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_install_hook_batch(
    targets: *mut Target,
    detours: *const Target,
    originals: *const *mut Target,
    count: u32,
    create: Create,
    enable: Hook,
    disable: Hook,
    retire: Hook,
) -> bool {
    if count == 0 {
        return true;
    }
    let count = count as usize;
    let (targets, detours, originals) = unsafe {
        (
            slice::from_raw_parts_mut(targets, count),
            slice::from_raw_parts(detours, count),
            slice::from_raw_parts(originals, count),
        )
    };
    let made = targets
        .iter()
        .zip(detours)
        .zip(originals)
        .take_while(|&((&target, &detour), &original)| {
            target.is_null() || unsafe { create(target, detour, original) }
        })
        .count();
    if made == count
        && targets
            .iter()
            .all(|&target| target.is_null() || unsafe { enable(target) })
    {
        return true;
    }
    let (owned, unowned) = targets.split_at_mut(made);
    unowned.fill(null_mut());
    remove(owned, disable, retire);
    false
}

/// The target array contains `count` entries for the duration of the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_remove_hook_batch(
    targets: *mut Target,
    count: u32,
    disable: Hook,
    retire: Hook,
) {
    if count != 0 {
        remove(
            unsafe { slice::from_raw_parts_mut(targets, count as usize) },
            disable,
            retire,
        );
    }
}
