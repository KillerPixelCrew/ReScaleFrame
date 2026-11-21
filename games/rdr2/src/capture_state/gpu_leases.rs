// SPDX-License-Identifier: MIT
//! COM ownership of the capture readbacks and their per-queue completion fences.

use super::{Capture, allocate};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{self, NonNull};

type RefCall = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
struct Ops {
    retain: RefCall,
    release: RefCall,
    create_fence: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    signal: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool,
}

#[repr(C)]
struct ReadbackLeases {
    buffer: Option<NonNull<c_void>>,
    fence: Option<NonNull<c_void>>,
    queue: Option<NonNull<c_void>>,
    ops: Option<&'static Ops>,
}

struct Com {
    raw: NonNull<c_void>,
    release: RefCall,
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { (self.release)(self.raw.as_ptr()) };
    }
}

struct Queue {
    fence: Com,
    // Live command queues cannot recycle their address under pending capture receipts.
    _queue: Com,
    next: u64,
}

#[derive(Default)]
pub(super) struct Queues(HashMap<usize, Queue>);

#[repr(C)]
struct Signal {
    fence: *mut c_void,
    value: u64,
}

const SIGNAL_ACCEPTED: u32 = 0;
const CREATE_REFUSED: u32 = 1;
const SIGNAL_REFUSED: u32 = 2;
const NO_SLOT: u32 = 3;

const _: () = {
    assert!(size_of::<ReadbackLeases>() == 32);
    assert!(size_of::<Ops>() == 32);
    assert!(size_of::<Signal>() == 16);
};

/// The native caller transfers the retained readback buffer and a process-lived leaf table.
#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_gpu_adopt(
    leases: &mut ReadbackLeases,
    buffer: *mut c_void,
    ops: &'static Ops,
) -> bool {
    let Some(buffer) = NonNull::new(buffer) else {
        return false;
    };
    if leases.buffer.is_some() {
        unsafe { (ops.release)(buffer.as_ptr()) };
        return false;
    }
    leases.ops = Some(ops);
    leases.buffer = Some(buffer);
    true
}

/// Retain the submitted queue until an accepted fence takes over its completion proof.
#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_gpu_bind_queue(leases: &mut ReadbackLeases, queue: *mut c_void) {
    let Some(ops) = leases.ops else { return };
    replace(&mut leases.queue, ops, NonNull::new(queue));
}

fn replace(slot: &mut Option<NonNull<c_void>>, ops: &Ops, object: Option<NonNull<c_void>>) {
    if let Some(object) = object {
        unsafe { (ops.retain)(object.as_ptr()) };
    }
    if let Some(previous) = std::mem::replace(slot, object) {
        unsafe { (ops.release)(previous.as_ptr()) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_gpu_bind_fence(leases: &mut ReadbackLeases, fence: *mut c_void) {
    let Some(ops) = leases.ops else { return };
    let fence = NonNull::new(fence);
    replace(&mut leases.fence, ops, fence);
    if fence.is_some() {
        // The queue owner now has a reference and the shot has the exact fence.
        replace(&mut leases.queue, ops, None);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_gpu_drop(leases: &mut ReadbackLeases) {
    rsf_rdr2_capture_gpu_bind_fence(leases, ptr::null_mut());
    rsf_rdr2_capture_gpu_bind_queue(leases, ptr::null_mut());
    if let (Some(ops), Some(buffer)) = (leases.ops.take(), leases.buffer.take()) {
        unsafe { (ops.release)(buffer.as_ptr()) };
    }
}

/// Called under the native capture mutex with the real queue that just submitted.
/// The returned fence is borrowed until the caller finishes assigning the receipt.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_gpu_signal(
    owner: &Capture,
    queue: *mut c_void,
    ops: &Ops,
    proof: &mut Signal,
) -> u32 {
    let Some(queue) = NonNull::new(queue) else {
        return CREATE_REFUSED;
    };
    let map = unsafe { &mut (*owner.queues.get()).0 };
    let key = queue.as_ptr().addr();
    if !map.contains_key(&key) {
        if map.try_reserve(1).is_err() {
            return NO_SLOT;
        }
        let Some(fence) = NonNull::new(unsafe { (ops.create_fence)(queue.as_ptr()) }) else {
            return CREATE_REFUSED;
        };
        unsafe { (ops.retain)(queue.as_ptr()) };
        map.insert(
            key,
            Queue {
                fence: Com {
                    raw: fence,
                    release: ops.release,
                },
                _queue: Com {
                    raw: queue,
                    release: ops.release,
                },
                next: 1,
            },
        );
    }
    let Some(state) = map.get_mut(&key) else {
        return NO_SLOT;
    };
    let value = state.next;
    // UINT64_MAX is reserved for device removal in GetCompletedValue.
    let Some(next) = value.checked_add(1) else {
        return NO_SLOT;
    };
    state.next = next;
    let fence = state.fence.raw.as_ptr();
    if !unsafe { (ops.signal)(queue.as_ptr(), fence, value) } {
        return SIGNAL_REFUSED;
    }
    *proof = Signal { fence, value };
    SIGNAL_ACCEPTED
}

/// The native caller has removed all pending readback receipts under its capture mutex.
/// Take the idle queue owners so their final Release calls run outside that mutex.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_gpu_take_queues(owner: &Capture) -> *mut Queues {
    let queues = unsafe { &mut *owner.queues.get() };
    if queues.0.is_empty() {
        return ptr::null_mut();
    }
    let retired = allocate(Queues::default());
    if !retired.is_null() {
        unsafe { ptr::swap(queues, retired) };
    }
    retired
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_gpu_drop_queues(retired: *mut Queues) {
    if !retired.is_null() {
        drop(unsafe { Box::from_raw(retired) });
    }
}
