// SPDX-License-Identifier: MIT
//! Pending capture shots and the poller's detached readback batch.
use super::gpu_leases::{
    Gpu, NO_SIGNAL_NEEDED, Ops, ReadbackLeases, SIGNAL_ACCEPTED, Signal, bind_queue,
    rsf_rdr2_capture_gpu_bind_fence, select_target, signal, take_idle,
};
use super::metadata::{InputId, RESOURCE_COUNT, RESOURCE_ROLES};
use super::recordings::{
    Draw, Pipeline, Readback, Submission, apply_submission, awaits_fence, claim_readback,
    fence_readback, reuse_readback, snapshot_submission, stamp_copy, submit_readback,
};
use super::{
    Capture, INPUT_FRAMES, KEEP_TAIL, MAX_DRAW_COPIES, SAVE_DRAW, commit_draw,
    rsf_rdr2_capture_active,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr;

const MAX_PENDING: usize = MAX_DRAW_COPIES as usize + INPUT_FRAMES as usize * RESOURCE_COUNT;
const REFUSED: u32 = 0;
const RECORDED: u32 = 1;
const NO_SLOT: u32 = 2;
const SAVE_EVENT: u32 = 3;

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
}

const _: () = {
    assert!(size_of::<Image>() == 160);
    assert!(std::mem::offset_of!(Image, pipeline) == 116);
};

pub(super) struct Shot {
    receipt: Readback,
    leases: ReadbackLeases,
    image: Image,
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

type Completed = unsafe extern "C" fn(*mut c_void) -> u64;
type Copy =
    unsafe extern "C" fn(*mut c_void, *mut c_void, u32, &mut ReadbackLeases, &mut Image) -> bool;
type WriteImage = unsafe extern "C" fn(&ReadbackLeases, &Image, *const c_char) -> bool;

impl Shot {
    fn completed(&self, sample: Completed) -> u64 {
        self.leases
            .fence
            .map_or(0, |fence| unsafe { sample(fence.as_ptr()) })
    }

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
pub(super) struct Ready {
    shots: Vec<Shot>,
    retired_tail: Option<Shot>,
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
    count: u32,
    lists: *const *mut c_void,
) -> *mut Submission {
    let submission = unsafe { snapshot_submission(owner, count, lists) };
    if let Some(submission) = unsafe { submission.as_ref() } {
        let pending = &mut unsafe { &mut *owner.shots.get() }.pending;
        for shot in pending {
            claim_readback(submission, &mut shot.receipt);
        }
    }
    submission
}

/// The native submission stack pairs the same queue and receipt after the original call.
/// A successful signal also covers older unfenced receipts already submitted to that queue.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_after_submit(
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_collect_ready(
    owner: &Capture,
    sample: Completed,
) -> *mut Gpu {
    if rsf_rdr2_capture_active(owner) {
        return ptr::null_mut();
    }
    let pending = &mut unsafe { &mut *owner.shots.get() }.pending;
    let ready = unsafe { &mut *owner.ready.get() };
    let mut index = 0;
    while let Some(shot) = pending.get(index) {
        if !shot.receipt.is_complete(shot.completed(sample)) {
            index += 1;
            continue;
        }
        if ready.shots.try_reserve(1).is_err() {
            break;
        }
        let mut shot = pending.remove(index);
        shot.finish_receipt();
        ready.shots.push(shot);
    }
    if pending.is_empty() {
        unsafe { take_idle(owner) }
    } else {
        ptr::null_mut()
    }
}

// Ready storage belongs to the existing worker. Writing and dropping it run outside the mutex.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_ready_count(owner: &Capture) -> usize {
    unsafe { &*owner.ready.get() }.shots.len()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_write_ready(
    owner: &Capture,
    index: usize,
    write: WriteImage,
) -> bool {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_drop_ready(owner: &Capture) {
    let ready = unsafe { &mut *owner.ready.get() };
    drop(ready.retired_tail.take());
    ready.shots.clear();
}
