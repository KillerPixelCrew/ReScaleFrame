// SPDX-License-Identifier: MIT
//! COM ownership of capture backbuffers, readbacks and per-queue completion fences.

use super::Capture;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{self, NonNull};

type RefCall = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
pub(super) struct Ops {
    pub(super) retain: RefCall,
    pub(super) release: RefCall,
    pub(super) create_fence: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub(super) signal: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool,
    pub(super) get_buffer: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct ReadbackLeases {
    pub(super) buffer: Option<NonNull<c_void>>,
    pub(super) fence: Option<NonNull<c_void>>,
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

const MAX_BUFFERS: usize = 4;

struct Backbuffers {
    buffers: [Option<Com>; MAX_BUFFERS],
    chain: Com,
    selected: Option<NonNull<c_void>>,
}

impl Backbuffers {
    fn accepts(&self, target: NonNull<c_void>) -> bool {
        self.selected.is_none_or(|selected| selected == target)
            && self
                .buffers
                .iter()
                .flatten()
                .any(|buffer| buffer.raw == target)
    }
}

#[derive(Default)]
pub(super) struct Gpu {
    queues: HashMap<usize, Queue>,
    backbuffers: Option<Backbuffers>,
}

pub(super) struct Signal {
    pub(super) fence: *mut c_void,
    pub(super) value: u64,
}

pub(super) const SIGNAL_ACCEPTED: u32 = 0;
pub(super) const CREATE_REFUSED: u32 = 1;
pub(super) const SIGNAL_REFUSED: u32 = 2;
const NO_SLOT: u32 = 3;
pub(super) const NO_SIGNAL_NEEDED: u32 = 4;

const _: () = {
    assert!(size_of::<ReadbackLeases>() == 32);
    assert!(size_of::<Ops>() == 40);
};

/// The native caller transfers the retained readback buffer and a process-lived leaf table.
#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_rdr2_capture_gpu_adopt(
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
pub(super) fn bind_queue(leases: &mut ReadbackLeases, queue: *mut c_void) {
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
pub(super) extern "C" fn rsf_rdr2_capture_gpu_bind_fence(
    leases: &mut ReadbackLeases,
    fence: *mut c_void,
) {
    let Some(ops) = leases.ops else { return };
    let fence = NonNull::new(fence);
    replace(&mut leases.fence, ops, fence);
    if fence.is_some() {
        // The queue owner now has a reference and the shot has the exact fence.
        replace(&mut leases.queue, ops, None);
    }
}

impl Drop for ReadbackLeases {
    fn drop(&mut self) {
        rsf_rdr2_capture_gpu_bind_fence(self, ptr::null_mut());
        bind_queue(self, ptr::null_mut());
        if let (Some(ops), Some(buffer)) = (self.ops.take(), self.buffer.take()) {
            unsafe { (ops.release)(buffer.as_ptr()) };
        }
    }
}

/// Called under the native capture mutex with the real queue that just submitted.
/// The returned fence is borrowed until the caller finishes assigning the receipt.
pub(super) unsafe fn signal(
    owner: &Capture,
    queue: *mut c_void,
    ops: &Ops,
    proof: &mut Signal,
) -> u32 {
    let Some(queue) = NonNull::new(queue) else {
        return CREATE_REFUSED;
    };
    let map = unsafe { &mut (*owner.gpu.get()).queues };
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

/// The native caller lends the admitted swapchain and exact buffer count under the capture mutex.
/// Keep any successful acquisitions local until the entire batch exists.
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_gpu_arm_backbuffers(
    owner: &Capture,
    swapchain: *mut c_void,
    count: u32,
    ops: &Ops,
) -> bool {
    let Some(chain) = NonNull::new(swapchain) else {
        return false;
    };
    let count = count as usize;
    let gpu = unsafe { &mut *owner.gpu.get() };
    if !(1..=MAX_BUFFERS).contains(&count) || gpu.backbuffers.is_some() {
        return false;
    }
    let mut buffers = [const { None }; MAX_BUFFERS];
    for (index, slot) in buffers[..count].iter_mut().enumerate() {
        let Some(raw) = NonNull::new(unsafe { (ops.get_buffer)(swapchain, index as u32) }) else {
            return false;
        };
        *slot = Some(Com {
            raw,
            release: ops.release,
        });
    }
    unsafe { (ops.retain)(chain.as_ptr()) };
    gpu.backbuffers = Some(Backbuffers {
        buffers,
        chain: Com {
            raw: chain,
            release: ops.release,
        },
        selected: None,
    });
    true
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_gpu_accepts_target(
    owner: &Capture,
    target: *mut c_void,
) -> bool {
    let gpu = unsafe { &*owner.gpu.get() };
    NonNull::new(target).is_some_and(|target| {
        gpu.backbuffers
            .as_ref()
            .is_some_and(|set| set.accepts(target))
    })
}

/// Commit the target only after the copy was recorded under the same capture mutex.
pub(super) unsafe fn select_target(owner: &Capture, target: *mut c_void) {
    let gpu = unsafe { &mut *owner.gpu.get() };
    if let (Some(set), Some(target)) = (&mut gpu.backbuffers, NonNull::new(target))
        && set.accepts(target)
    {
        set.selected = Some(target);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_gpu_selected(
    owner: &Capture,
    swapchain: *mut c_void,
) -> *mut c_void {
    unsafe { &*owner.gpu.get() }
        .backbuffers
        .as_ref()
        .filter(|set| set.chain.raw.as_ptr() == swapchain)
        .and_then(|set| set.selected)
        .map_or(ptr::null_mut(), NonNull::as_ptr)
}

/// Ready admission has removed all pending readback receipts under the native capture mutex.
/// Take the idle queue and swapchain owners so their final Release calls run outside that mutex.
#[cfg(any(windows, test))]
pub(super) unsafe fn take_idle(owner: &Capture) -> Option<Gpu> {
    let gpu = unsafe { &mut *owner.gpu.get() };
    if gpu.queues.is_empty() && gpu.backbuffers.is_none() {
        return None;
    }
    Some(std::mem::take(gpu))
}
