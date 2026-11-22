// SPDX-License-Identifier: MIT
//! Pending capture shots and the poller's detached readback batch.
#[cfg(any(windows, test))]
use super::gpu_leases::{Gpu, take_idle};
use super::gpu_leases::{
    NO_SIGNAL_NEEDED, Ops, ReadbackLeases, SIGNAL_ACCEPTED, Signal, bind_queue,
    rsf_rdr2_capture_gpu_bind_fence, select_target, signal,
};
use super::metadata::{InputId, RESOURCE_COUNT, RESOURCE_ROLES};
use super::recordings::{
    Draw, Pipeline, Readback, Submission, apply_submission, awaits_fence, claim_readback,
    discard_submission, fence_readback, reuse_readback, snapshot_submission, stamp_copy,
    submit_readback,
};
use super::{
    Capture, INPUT_FRAMES, KEEP_TAIL, MAX_DRAW_COPIES, SAVE_DRAW, commit_draw,
    rsf_rdr2_capture_active,
};
use std::cell::RefCell;
#[cfg(windows)]
use std::ffi::c_char;
use std::ffi::{CStr, c_void};
use std::ptr;

const MAX_PENDING: usize = MAX_DRAW_COPIES as usize + INPUT_FRAMES as usize * RESOURCE_COUNT;
const REFUSED: u32 = 0;
const RECORDED: u32 = 1;
const NO_SLOT: u32 = 2;
const SAVE_EVENT: u32 = 3;
const UNPAIRED_SUBMISSION: u32 = 5;

struct QueueCall {
    queue: *mut c_void,
    snapshot: *mut Submission,
}

impl Drop for QueueCall {
    fn drop(&mut self) {
        unsafe { discard_submission(self.snapshot) };
    }
}

struct QueueCalls {
    stack: Vec<QueueCall>,
    refused: usize,
}

thread_local! {
    static QUEUE_CALLS: RefCell<QueueCalls> = const { RefCell::new(QueueCalls {
        stack: Vec::new(), refused: 0,
    }) };
}

#[repr(C)]
#[derive(Default)]
pub(super) struct Image {
    pub(super) list: u64,
    pub(super) generation: u64,
    pub(super) submission: u64,
    pub(super) queue: u64,
    pub(super) fence_value: u64,
    pub(super) tick: u64,
    pub(super) resource: u64,
    pub(super) caller: u64,
    pub(super) sequence: u32,
    pub(super) ordinal: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pitch: u32,
    pub(super) format: u32,
    pub(super) input_index: u32,
    pub(super) footprint_format: u32,
    pub(super) state_before: u32,
    pub(super) subresource: u32,
    pub(super) frame: u32,
    pub(super) vertices: u32,
    pub(super) instances: u32,
    pub(super) pipeline: Pipeline,
    pub(super) pipeline_known: bool,
    pub(super) pso: u64,
}

const _: () = {
    assert!(size_of::<Image>() == 168);
    assert!(std::mem::offset_of!(Image, pipeline) == 116);
};

pub(super) struct Shot {
    receipt: Readback,
    leases: ReadbackLeases,
    image: Image,
    #[cfg_attr(not(windows), allow(dead_code))]
    role: &'static CStr,
    tail: bool,
}

impl Default for Shot {
    fn default() -> Self {
        Self {
            receipt: Readback::default(),
            leases: ReadbackLeases::default(),
            image: Image::default(),
            role: c"draw",
            tail: false,
        }
    }
}

pub(super) type Completed = unsafe extern "C" fn(*mut c_void) -> u64;
#[cfg(any(windows, test))]
pub(super) type DeviceRemoved = unsafe extern "C" fn(*mut c_void) -> bool;
type Copy =
    unsafe extern "C" fn(*mut c_void, *mut c_void, u32, &mut ReadbackLeases, &mut Image) -> bool;
#[cfg(windows)]
pub(super) type WriteImage = unsafe extern "C" fn(&ReadbackLeases, &Image, *const c_char) -> bool;

impl Shot {
    fn completed(&self, sample: Completed) -> u64 {
        self.leases
            .fence
            .map_or(0, |fence| unsafe { sample(fence.as_ptr()) })
    }

    #[cfg(any(windows, test))]
    fn finish_receipt(&mut self) {
        self.image.list = self.receipt.list.addr() as u64;
        self.image.generation = self.receipt.generation;
        self.image.submission = self.receipt.submission;
        self.image.queue = self.receipt.queue.addr() as u64;
        self.image.fence_value = self.receipt.fence_value;
    }
}

#[derive(Default)]
pub(super) struct Shots {
    pub(super) pending: Vec<Shot>,
    // When the ready batch first writes a tail, it can outlive that batch until publication.
    #[cfg_attr(not(windows), allow(dead_code))]
    retained_tail: Option<Shot>,
}

#[derive(Default)]
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(super) struct Ready {
    shots: Vec<Shot>,
    failed: Vec<Shot>,
    retired_tail: Option<Shot>,
    #[cfg(windows)]
    pub(super) bundle: Option<super::bundle::Snapshot>,
}

impl Shots {
    fn push(&mut self, shot: Shot) -> Option<&mut Shot> {
        if self.pending.len() >= MAX_PENDING || self.pending.try_reserve(1).is_err() {
            return None;
        }
        self.pending.push(shot);
        self.pending.last_mut()
    }
}

pub(super) unsafe fn discard_reset(owner: &Capture, list: *mut c_void, generation: u64) {
    for shot in &mut unsafe { &mut *owner.shots.get() }.pending {
        if shot.receipt.discarded_by_reset(list, generation) {
            // Keep the COM release on the poller, outside the graphics mutex.
            shot.receipt.discarded = true;
        }
    }
}

/// When the original queue call must run without a receipt, its copies cannot be proven
/// unsubmitted by a subsequent Reset. A confirmed device loss can still retire them.
unsafe fn untracked_submit(owner: &Capture, count: u32, lists: *const *mut c_void) {
    if count == 0 {
        return;
    }
    let lists =
        (!lists.is_null()).then(|| unsafe { std::slice::from_raw_parts(lists, count as usize) });
    for shot in &mut unsafe { &mut *owner.shots.get() }.pending {
        if !shot.receipt.discarded
            && shot.receipt.submission == 0
            && lists.is_none_or(|lists| lists.contains(&shot.receipt.list))
        {
            shot.receipt.untracked = true;
        }
    }
}

/// Under the native capture mutex, put the lease in pending before the native copy can
/// record any commands. A refused copy can drop the new slot immediately.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_input_shot(
    owner: &Capture,
    id: &InputId,
    role: u32,
    list: *mut c_void,
    resource: *mut c_void,
    before: u32,
    copy: Copy,
) -> u32 {
    let Some(&role) = RESOURCE_ROLES.get(role as usize) else {
        return REFUSED;
    };
    let shots = unsafe { &mut *owner.shots.get() };
    let Some(shot) = shots.push(Shot {
        image: Image {
            sequence: id.sequence,
            input_index: id.index,
            ..Image::default()
        },
        role,
        ..Shot::default()
    }) else {
        return NO_SLOT;
    };
    if !unsafe { copy(list, resource, before, &mut shot.leases, &mut shot.image) } {
        shots.pending.pop();
        return REFUSED;
    }
    unsafe { stamp_copy(owner, list, &mut shot.receipt) };
    RECORDED
}

/// The caller stays on the render thread and records the stack at the native draw site
/// when this returns SAVE_EVENT. A draw with overflowing metadata can still record its copy.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_draw_shot(
    owner: &Capture,
    list: *mut c_void,
    resource: *mut c_void,
    before: u32,
    copy: Copy,
    sample: Completed,
    draw: &mut Draw,
) -> u32 {
    let (sequence, ordinal) = {
        let ledger = unsafe { &*owner.ledger.get() };
        (ledger.sequence, ledger.draws)
    };
    let shots = unsafe { &mut *owner.shots.get() };
    let reuse = ordinal >= MAX_DRAW_COPIES;
    let shot = if reuse {
        let Some(shot) = shots.pending.iter_mut().rfind(|shot| shot.tail) else {
            return REFUSED;
        };
        if !unsafe { reuse_readback(owner, &shot.receipt, list, shot.completed(sample)) } {
            return REFUSED;
        }
        shot
    } else {
        let Some(shot) = shots.push(Shot::default()) else {
            return NO_SLOT;
        };
        shot
    };
    if !unsafe { copy(list, resource, before, &mut shot.leases, &mut shot.image) } {
        if !reuse {
            shots.pending.pop();
        }
        return REFUSED;
    }
    unsafe { stamp_copy(owner, list, &mut shot.receipt) };
    shot.image.sequence = sequence;
    shot.image.ordinal = ordinal;
    shot.image.vertices = draw.vertices;
    shot.image.instances = draw.instances;
    shot.image.caller = draw.caller;
    shot.image.pipeline = draw.pipeline;
    shot.image.pipeline_known = draw.pipeline_known;
    shot.image.pso = draw.pso;
    unsafe { select_target(owner, resource) };
    let action = unsafe { commit_draw(owner) };
    shot.tail |= action & KEEP_TAIL != 0;
    if action & SAVE_DRAW == 0 {
        return RECORDED;
    }
    draw.ordinal = ordinal;
    draw.frame = shot.image.frame;
    draw.generation = shot.receipt.generation;
    SAVE_EVENT
}

/// Claim only the pending shots proved by the snapshot taken immediately before the
/// original queue call. An in-flight claim keeps that shot pending and unavailable for reuse.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_before_submit(
    owner: &Capture,
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    QUEUE_CALLS.with_borrow_mut(|calls| {
        if calls.refused != 0 || calls.stack.try_reserve(1).is_err() {
            calls.refused += 1;
            unsafe { untracked_submit(owner, count, lists) };
        } else {
            let snapshot = unsafe { claim_submission(owner, count, lists) };
            calls.stack.push(QueueCall { queue, snapshot });
        }
    });
}

unsafe fn claim_submission(
    owner: &Capture,
    count: u32,
    lists: *const *mut c_void,
) -> *mut Submission {
    let submission = unsafe { snapshot_submission(owner, count, lists) };
    if let Some(submission) = unsafe { submission.as_ref() } {
        let pending = &mut unsafe { &mut *owner.shots.get() }.pending;
        for shot in pending {
            claim_readback(submission, &mut shot.receipt);
        }
    } else {
        unsafe { untracked_submit(owner, count, lists) };
    }
    submission
}

/// The native submission stack pairs the same queue and receipt after the original call.
/// A successful signal also covers older unfenced receipts already submitted to that queue.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_after_submit(
    owner: &Capture,
    queue: *mut c_void,
    ops: &Ops,
) -> u32 {
    // Release the TLS borrow before signalling: the native leaf can reenter the observer.
    let call = QUEUE_CALLS.with_borrow_mut(|calls| {
        if calls.refused != 0 {
            calls.refused -= 1;
            Ok(None)
        } else if calls.stack.last().is_some_and(|call| call.queue == queue) {
            Ok(calls.stack.pop())
        } else {
            Err(UNPAIRED_SUBMISSION)
        }
    });
    let call = match call {
        Ok(call) => call,
        Err(result) => return result,
    };
    let snapshot = call.as_ref().map_or(ptr::null_mut(), |call| call.snapshot);
    unsafe { finish_submission(owner, queue, snapshot, ops) }
}

unsafe fn finish_submission(
    owner: &Capture,
    queue: *mut c_void,
    submission: *mut Submission,
    ops: &Ops,
) -> u32 {
    let serial = unsafe { apply_submission(owner, submission) };
    let pending = &mut unsafe { &mut *owner.shots.get() }.pending;
    if !submission.is_null() {
        for shot in &mut *pending {
            if submit_readback(&mut shot.receipt, queue, serial) {
                bind_queue(&mut shot.leases, queue);
            }
        }
    }
    if !pending
        .iter()
        .any(|shot| awaits_fence(&shot.receipt, queue))
    {
        return NO_SIGNAL_NEEDED;
    }
    let mut proof = Signal {
        fence: ptr::null_mut(),
        value: 0,
    };
    let result = unsafe { signal(owner, queue, ops, &mut proof) };
    if result == SIGNAL_ACCEPTED {
        for shot in pending {
            if fence_readback(&mut shot.receipt, queue, proof.value) {
                rsf_rdr2_capture_gpu_bind_fence(&mut shot.leases, proof.fence);
            }
        }
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_pending_count(owner: &Capture) -> usize {
    unsafe { &*owner.shots.get() }.pending.len()
}

/// Only the poller calls this, under the native capture mutex. Transfer completed shots in
/// pending order. Allocation refusal leaves the current shot pending for the next poll.
#[cfg(any(windows, test))]
pub(super) unsafe fn collect_ready(
    owner: &Capture,
    sample: Completed,
    removed: DeviceRemoved,
) -> *mut Gpu {
    if rsf_rdr2_capture_active(owner) {
        return ptr::null_mut();
    }
    let pending = &mut unsafe { &mut *owner.shots.get() }.pending;
    let ready = unsafe { &mut *owner.ready.get() };
    let mut index = 0;
    while let Some(shot) = pending.get(index) {
        let value = shot.completed(sample);
        let lost = (value == u64::MAX
            || (shot.leases.fence.is_none()
                && (shot.receipt.submission != 0 || shot.receipt.untracked)))
            && shot
                .leases
                .buffer
                .is_some_and(|buffer| unsafe { removed(buffer.as_ptr()) });
        let unavailable = shot.receipt.discarded || lost;
        if !unavailable && !shot.receipt.is_complete(value) {
            index += 1;
            continue;
        }
        let destination = if unavailable {
            &mut ready.failed
        } else {
            &mut ready.shots
        };
        if destination.try_reserve(1).is_err() {
            break;
        }
        let mut shot = pending.remove(index);
        if unavailable {
            unsafe { (*owner.ledger.get()).readbacks_unavailable += 1 };
        } else {
            shot.finish_receipt();
        }
        destination.push(shot);
    }
    if pending.is_empty() {
        unsafe { take_idle(owner) }
    } else {
        ptr::null_mut()
    }
}

// Ready storage belongs to the existing worker. Writing and dropping it run outside the mutex.
#[cfg(any(windows, test))]
pub(super) unsafe fn ready_count(owner: &Capture) -> usize {
    unsafe { &*owner.ready.get() }.shots.len()
}

#[cfg(windows)]
pub(super) unsafe fn write_ready(owner: &Capture, index: usize, write: WriteImage) -> bool {
    let ready = unsafe { &*owner.ready.get() };
    ready
        .shots
        .get(index)
        .is_some_and(|shot| unsafe { write(&shot.leases, &shot.image, shot.role.as_ptr()) })
}

/// The worker settles the tail under the capture mutex after accounting for the batch.
/// An already written tail stays retained across polls until a publication attempt.
#[cfg(windows)]
pub(super) unsafe fn settle_tail(owner: &Capture, published: bool) {
    let ready = unsafe { &mut *owner.ready.get() };
    let shots = unsafe { &mut *owner.shots.get() };
    if published {
        ready.retired_tail = shots.retained_tail.take();
    } else if let Some(index) = ready.shots.iter().position(|shot| shot.tail) {
        shots.retained_tail = Some(ready.shots.remove(index));
    }
}

#[cfg(any(windows, test))]
pub(super) unsafe fn drop_ready(owner: &Capture) {
    let ready = unsafe { &mut *owner.ready.get() };
    drop(ready.retired_tail.take());
    ready.shots.clear();
    ready.failed.clear();
}

#[cfg(test)]
mod tests {
    use super::super::gpu_leases::rsf_rdr2_capture_gpu_adopt;
    use super::super::recordings::rsf_rdr2_capture_reset_list;
    use super::*;
    use std::ptr::without_provenance_mut;

    unsafe extern "C" fn retain_or_release(_: *mut c_void) {}
    unsafe extern "C" fn no_fence(_: *mut c_void) -> *mut c_void {
        ptr::null_mut()
    }
    unsafe extern "C" fn no_signal(_: *mut c_void, _: *mut c_void, _: u64) -> bool {
        false
    }
    unsafe extern "C" fn no_buffer(_: *mut c_void, _: u32) -> *mut c_void {
        ptr::null_mut()
    }
    unsafe extern "C" fn completed(_: *mut c_void) -> u64 {
        u64::MAX
    }
    unsafe extern "C" fn device_live(_: *mut c_void) -> bool {
        false
    }
    unsafe extern "C" fn device_removed(_: *mut c_void) -> bool {
        true
    }

    #[test]
    fn reset_retires_only_the_unclaimed_generation() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let other = without_provenance_mut(2);
        unsafe {
            let pending = &mut (*owner.shots.get()).pending;
            for list in [list, other, list] {
                let mut shot = Shot::default();
                stamp_copy(&owner, list, &mut shot.receipt);
                pending.push(shot);
            }
            let submission = snapshot_submission(&owner, 1, [list].as_ptr());
            assert!(!submission.is_null());
            assert!(claim_readback(&*submission, &mut pending[0].receipt));
            untracked_submit(&owner, 1, [other].as_ptr());
            rsf_rdr2_capture_reset_list(&owner, list, ptr::null_mut());
            assert!(collect_ready(&owner, completed, device_live).is_null());
            assert_eq!(rsf_rdr2_capture_pending_count(&owner), 2);
            assert_eq!(ready_count(&owner), 0);
            assert_eq!((*owner.ledger.get()).readbacks_unavailable, 1);
            drop(Box::from_raw(submission));
            drop_ready(&owner);
        }
    }

    #[test]
    fn a_removal_sentinel_requires_device_confirmation_and_never_admits_a_mapping() {
        static OPS: Ops = Ops {
            retain: retain_or_release,
            release: retain_or_release,
            create_fence: no_fence,
            signal: no_signal,
            get_buffer: no_buffer,
        };
        let owner = Capture::new();
        let mut shot = Shot::default();
        assert!(rsf_rdr2_capture_gpu_adopt(
            &mut shot.leases,
            without_provenance_mut(1),
            &OPS
        ));
        rsf_rdr2_capture_gpu_bind_fence(&mut shot.leases, without_provenance_mut(2));
        shot.receipt.fence_value = 1;
        unsafe {
            (*owner.shots.get()).pending.push(shot);
            assert!(collect_ready(&owner, completed, device_live).is_null());
            assert_eq!(rsf_rdr2_capture_pending_count(&owner), 1);
            assert!(collect_ready(&owner, completed, device_removed).is_null());
            assert_eq!(rsf_rdr2_capture_pending_count(&owner), 0);
            assert_eq!(ready_count(&owner), 0);
            assert_eq!((*owner.ledger.get()).readbacks_unavailable, 1);
            drop_ready(&owner);
        }
    }
}
