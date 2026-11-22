// SPDX-License-Identifier: MIT
//! Presentation sequencing for a Godot batch containing a proxy swapchain.

use core::ffi::c_void;

const FEATURE_NOT_PRESENT: i32 = -8;

#[repr(C)]
pub struct MixedPresentCalls {
    fan_out: unsafe extern "C" fn(context: *mut c_void) -> i32,
    present: unsafe extern "C" fn(context: *mut c_void, index: u32) -> i32,
}

/// # Safety
/// `calls` and `context` must remain valid until the batch returns. `count` is the number
/// of valid chains and result slots in the borrowed present request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_present_mixed(
    calls: *const MixedPresentCalls,
    context: *mut c_void,
    count: u32,
    has_extensions: u32,
    submitted: u32,
) -> i32 {
    if has_extensions != 0 {
        return FEATURE_NOT_PRESENT;
    }
    let calls = unsafe { &*calls };
    if submitted == 0 {
        // Consume the batch's binary waits once before distributing each chain's signal.
        let result = unsafe { (calls.fan_out)(context) };
        if result != 0 {
            return result;
        }
    }

    let mut result = 0;
    for index in 0..count {
        let item = unsafe { (calls.present)(context, index) };
        if item != 0 {
            result = item;
        }
    }
    result
}
