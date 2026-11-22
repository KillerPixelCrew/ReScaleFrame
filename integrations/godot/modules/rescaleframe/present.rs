// SPDX-License-Identifier: MIT
//! Vulkan submission receipts and presentation sequencing for Godot's queues.

use core::{
    ffi::c_void,
    mem::{offset_of, size_of},
};

use crate::{frame, host};

const FEATURE_NOT_PRESENT: i32 = -8;

#[repr(C)]
pub struct ExecutionReceipt {
    queue: u64,
    fence: u64,
    session: u64,
    source: u64,
    submitted: u32,
}

const _: () = {
    assert!(size_of::<ExecutionReceipt>() == 40);
    assert!(offset_of!(ExecutionReceipt, submitted) == 32);
};

// The C++ RAII scope provides thread-local ownership, including nested scopes and the
// engine's separate present queue. It reports only the verified submit boundaries.
/// # Safety
/// `receipt` must be writable, aligned storage in the current engine execution scope.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_execution_start(
    receipt: *mut ExecutionReceipt,
    queue: u64,
    fence: u64,
    supported: u32,
) {
    let (source, session) = if supported != 0 && fence != 0 {
        (frame::render_source(), host::rsf_godot_host_session())
    } else {
        (0, 0)
    };
    unsafe {
        receipt.write(ExecutionReceipt {
            queue,
            fence,
            session,
            source,
            submitted: 0,
        })
    };
}

/// # Safety
/// `receipt` must be the initialized receipt from the current engine execution scope.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_execution_submit(
    receipt: *mut ExecutionReceipt,
    queue: u64,
    fence: u64,
    begin: u32,
) -> u32 {
    let receipt = unsafe { &mut *receipt };
    if receipt.session == 0
        || receipt.source == 0
        || receipt.queue != queue
        || receipt.fence != fence
    {
        return 0;
    }
    receipt.submitted = (begin == 0) as u32;
    1
}

/// # Safety
/// `receipt` must be the initialized receipt from the current engine execution scope.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_execution_present(receipt: *const ExecutionReceipt) -> u32 {
    unsafe { (*receipt).submitted }
}

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
