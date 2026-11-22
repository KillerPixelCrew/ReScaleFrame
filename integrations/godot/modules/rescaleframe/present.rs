// SPDX-License-Identifier: MIT
//! Vulkan submission receipts and presentation sequencing for Godot's queues.

use core::{
    ffi::c_void,
    mem::{offset_of, size_of},
};

use crate::packet_abi::{
    RSF_GAME_RENDER_PRESENT, RSF_GAME_RENDER_SUBMISSION, rsf_godot_present_calls,
};
use crate::{frame, host};

type ExecutionEvent = unsafe extern "C" fn(u32, u64, u64, u64, u32) -> u32;
type LegacyMarker = unsafe extern "C" fn(u64, u32) -> u32;

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
/// A non-null receipt must be initialized and borrowed from the current engine scope.
/// The event callback must remain callable for this synchronous report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_execution_submit(
    receipt: *mut ExecutionReceipt,
    queue: u64,
    fence: u64,
    begin: u32,
    event: ExecutionEvent,
) {
    let Some(receipt) = (unsafe { receipt.as_mut() }) else {
        return;
    };
    if receipt.session == 0
        || receipt.source == 0
        || receipt.queue != queue
        || receipt.fence != fence
    {
        return;
    }
    receipt.submitted = (begin == 0) as u32;
    let (session, source) = (receipt.session, receipt.source);
    unsafe { event(RSF_GAME_RENDER_SUBMISSION, session, source, 0, begin) };
}

/// # Safety
/// A non-null receipt must be initialized and borrowed from the current engine scope.
/// Both callbacks must remain callable for this synchronous report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_execution_present(
    receipt: *const ExecutionReceipt,
    supported: u32,
    window: u64,
    begin: u32,
    event: ExecutionEvent,
    legacy: LegacyMarker,
) -> u32 {
    if supported == 0 {
        return unsafe { legacy(window, begin) };
    }
    let Some(receipt) = (unsafe { receipt.as_ref() }) else {
        return 0;
    };
    if receipt.submitted == 0 {
        return 0;
    }
    let (session, source) = (receipt.session, receipt.source);
    unsafe { event(RSF_GAME_RENDER_PRESENT, session, source, window, begin) }
}

/// # Safety
/// The native present request, all chain and result slots, and every callback must
/// remain valid until this call returns. All callbacks must be nonnull.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_present_batch(
    calls: *const rsf_godot_present_calls,
    context: *mut c_void,
    count: u32,
    has_extensions: u32,
    submitted: u32,
) -> i32 {
    let calls = unsafe { &*calls };
    let proxy_at = unsafe { calls.proxy_at.unwrap_unchecked() };
    let mut proxy = false;
    for index in 0..count {
        proxy |= unsafe { proxy_at(context, index) != 0 };
    }
    if proxy && count != 1 {
        unsafe { calls.trace_mixed.unwrap_unchecked()(context) };
        if has_extensions != 0 {
            return FEATURE_NOT_PRESENT;
        }
        if submitted == 0 {
            // Consume the batch's binary waits once before distributing each chain's signal.
            let result = unsafe { calls.fan_out.unwrap_unchecked()(context) };
            if result != 0 {
                return result;
            }
        }

        let present_item = unsafe { calls.present_item.unwrap_unchecked() };
        let mut result = 0;
        for index in 0..count {
            let item = unsafe { present_item(context, index) };
            if item != 0 {
                result = item;
            }
        }
        return result;
    }

    // Native batches report the first surface whose real receipt admits a marker.
    // A one-chain proxy uses the same scope around its direct native host call.
    let window_at = unsafe { calls.window_at.unwrap_unchecked() };
    let marker = unsafe { calls.marker.unwrap_unchecked() };
    let mut marked_window = 0;
    for index in 0..count {
        let mut window = 0;
        if unsafe { window_at(context, index, &mut window) != 0 && marker(window, 1) != 0 }
            && window != 0
        {
            marked_window = window;
            break;
        }
    }
    let present = unsafe {
        if proxy {
            calls.proxy_batch.unwrap_unchecked()
        } else {
            calls.native_batch.unwrap_unchecked()
        }
    };
    let result = unsafe { present(context) };
    if marked_window != 0 {
        unsafe { marker(marked_window, 0) };
    }
    result
}
