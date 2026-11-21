// SPDX-License-Identifier: MIT
//! Command-list observations and copied draw facts ordered by the native capture mutex.
use super::{Capture, MAX_DRAW_METADATA, allocate};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

const UNKNOWN: u32 = u32::MAX;
type States = HashMap<usize, u32>;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Pipeline {
    pub(super) ps: u32,
    pub(super) vs: u32,
    pub(super) blend: i32,
    pub(super) src: u32,
    pub(super) dst: u32,
    pub(super) op: u32,
    pub(super) src_alpha: u32,
    pub(super) dst_alpha: u32,
    pub(super) op_alpha: u32,
    pub(super) write_mask: u32,
}

#[repr(C)]
struct DrawBinding {
    target: *mut c_void,
    pso: u64,
    pipeline: Pipeline,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Default))]
pub(super) struct Draw {
    pub(super) ordinal: u32,
    pub(super) frame: u32,
    pub(super) vertices: u32,
    pub(super) instances: u32,
    pub(super) list: u64,
    pub(super) generation: u64,
    pub(super) caller: u64,
    pub(super) pso: u64,
    pub(super) pipeline: Pipeline,
    pub(super) stack: [*mut c_void; 12],
    pub(super) stack_size: u32,
}

const _: () = {
    assert!(size_of::<Pipeline>() == 40);
    assert!(size_of::<DrawBinding>() == 56 && std::mem::offset_of!(DrawBinding, pipeline) == 16);
    assert!(size_of::<Draw>() == 192 && std::mem::offset_of!(Draw, stack) == 88);
};

#[derive(Default)]
struct Recording {
    generation: u64,
    states: States,
    target: *mut c_void,
    pso: usize,
}

#[derive(Default)]
pub(super) struct Recordings {
    lists: HashMap<usize, Recording>,
    rtvs: HashMap<usize, *mut c_void>,
    pipelines: HashMap<usize, Pipeline>,
    submitted: States,
    submission_serial: u64,
    pub(super) draws: Vec<Draw>,
}

pub(super) struct Submission {
    pub(super) serial: u64,
    generations: HashMap<usize, u64>,
    states: States,
}

#[derive(Default)]
pub(super) struct Readback {
    pub(super) generation: u64,
    pub(super) submission: u64,
    pub(super) fence_value: u64,
    pub(super) list: *mut c_void,
    pub(super) queue: *mut c_void,
    claimed: Option<u64>,
}

impl Readback {
    pub(super) fn is_complete(&self, completed: u64) -> bool {
        self.fence_value != 0 && completed != u64::MAX && completed >= self.fence_value
    }
}

fn try_entry<V: Default>(map: &mut HashMap<usize, V>, key: usize) -> Option<&mut V> {
    if !map.contains_key(&key) && map.try_reserve(1).is_err() {
        return None;
    }
    Some(map.entry(key).or_default())
}

impl Recordings {
    pub(super) fn generation(&self, list: usize) -> u64 {
        self.lists
            .get(&list)
            .map_or(0, |recording| recording.generation)
    }

    fn state(&self, list: usize, resource: usize) -> u32 {
        self.lists
            .get(&list)
            .and_then(|recording| recording.states.get(&resource))
            .or_else(|| self.submitted.get(&resource))
            .copied()
            .unwrap_or(UNKNOWN)
    }

    fn record(&mut self, list: usize, resource: usize, state: u32) {
        if let Some(value) = try_entry(&mut self.lists, list)
            .and_then(|recording| try_entry(&mut recording.states, resource))
        {
            *value = state;
        } else {
            // If a transition cannot be recorded, its prior submitted state is no proof.
            self.submitted.remove(&resource);
        }
    }

    fn snapshot(&mut self, lists: &[*mut c_void]) -> Option<Submission> {
        // A claim can outlive a refused or abandoned submission. Never reuse its serial.
        self.submission_serial = self.submission_serial.checked_add(1)?;
        let mut submission = Submission {
            serial: self.submission_serial,
            generations: HashMap::new(),
            states: States::new(),
        };
        submission.generations.try_reserve(lists.len()).ok()?;
        for &list in lists {
            let list = list as usize;
            if let Some(recording) = self.lists.get(&list) {
                submission.generations.insert(list, recording.generation);
                submission.states.try_reserve(recording.states.len()).ok()?;
                // Last recording in submission order wins for states observed on several lists.
                submission
                    .states
                    .extend(recording.states.iter().map(|(&key, &value)| (key, value)));
            }
        }
        Some(submission)
    }

    fn submit(&mut self, submission: &mut Submission) -> u64 {
        if self.submitted.try_reserve(submission.states.len()).is_ok() {
            self.submitted.extend(submission.states.drain());
        } else {
            // Do not serve stale global facts after a submission we could not track.
            self.submitted.clear();
        }
        submission.serial
    }
}

// The native descriptor parser lends the exact shader bytes for this call. Hashing does
// not access the owner and runs before the native graphics mutex is taken.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_hash_shader(
    bytes: *const u8,
    length: usize,
    out: &mut u32,
) -> bool {
    if length == 0 {
        *out = 0;
        return true;
    }
    if bytes.is_null() || length > isize::MAX as usize {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
    let mut value = u32::MAX;
    for &byte in bytes {
        value ^= u32::from(byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(value & 1));
        }
    }
    *out = !value;
    true
}

// All of these calls run under g->mutex. Resource and command-list addresses are identities.
// The observation maps do not own COM references.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_record_pipeline(
    owner: &Capture,
    pso: *mut c_void,
    facts: *const Pipeline,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    let key = pso as usize;
    let Some(facts) = (unsafe { facts.as_ref() }) else {
        // A successful callback with an unrecognized descriptor supersedes an old address.
        recordings.pipelines.remove(&key);
        return;
    };
    if let Some(pipeline) = try_entry(&mut recordings.pipelines, key) {
        *pipeline = *facts;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_record_rtv(
    owner: &Capture,
    handle: usize,
    resource: *mut c_void,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if resource.is_null() {
        recordings.rtvs.remove(&handle);
    } else if let Some(target) = try_entry(&mut recordings.rtvs, handle) {
        *target = resource;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_bind_rtv(
    owner: &Capture,
    list: *mut c_void,
    handle: *const usize,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    // Resolve the first view at bind time. A later descriptor write does not retarget the list.
    let target = unsafe { handle.as_ref() }
        .and_then(|handle| recordings.rtvs.get(handle))
        .copied()
        .unwrap_or(null_mut());
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.target = target;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_bind_pipeline(
    owner: &Capture,
    list: *mut c_void,
    pso: *mut c_void,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.pso = pso as usize;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_get_draw_binding(
    owner: &Capture,
    list: *mut c_void,
    out: &mut DrawBinding,
) -> bool {
    let recordings = unsafe { &*owner.recordings.get() };
    let Some(recording) = recordings.lists.get(&(list as usize)) else {
        return false;
    };
    if recording.target.is_null() {
        return false;
    }
    *out = DrawBinding {
        target: recording.target,
        pso: recording.pso as u64,
        pipeline: recordings
            .pipelines
            .get(&recording.pso)
            .copied()
            .unwrap_or_default(),
    };
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_record_draw(owner: &Capture, draw: &Draw) -> bool {
    let recordings = unsafe { &mut *owner.recordings.get() };
    let draws = &mut recordings.draws;
    if draws.len() >= MAX_DRAW_METADATA || draws.try_reserve(1).is_err() {
        return false;
    }
    draws.push(*draw);
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_initial_state(
    owner: &Capture,
    resource: *mut c_void,
    state: u32,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(value) = try_entry(&mut recordings.submitted, resource as usize) {
        *value = state;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_transition(
    owner: &Capture,
    list: *mut c_void,
    resource: *mut c_void,
    subresource: u32,
    after: u32,
    begin_only: bool,
) {
    // Probe copies read subresource zero; another plane or mip does not establish its state.
    if matches!(subresource, 0 | u32::MAX) {
        unsafe { &mut *owner.recordings.get() }.record(
            list as usize,
            resource as usize,
            if begin_only { UNKNOWN } else { after },
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_alias(
    owner: &Capture,
    list: *mut c_void,
    resource: *mut c_void,
) {
    if !resource.is_null() {
        unsafe { &mut *owner.recordings.get() }.record(list as usize, resource as usize, UNKNOWN);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_reset_list(
    owner: &Capture,
    list: *mut c_void,
    pso: *mut c_void,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.generation = recording.generation.wrapping_add(1);
        recording.states.clear();
        recording.target = null_mut();
        recording.pso = pso as usize;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_resource_state(
    owner: &Capture,
    list: *mut c_void,
    resource: *mut c_void,
) -> u32 {
    unsafe { &*owner.recordings.get() }.state(list as usize, resource as usize)
}

pub(super) unsafe fn stamp_copy(owner: &Capture, list: *mut c_void, readback: &mut Readback) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    let generation =
        try_entry(&mut recordings.lists, list as usize).map_or(0, |recording| recording.generation);
    *readback = Readback {
        generation,
        list,
        ..Readback::default()
    };
}

pub(super) unsafe fn reuse_readback(
    owner: &Capture,
    readback: &Readback,
    list: *mut c_void,
    completed: u64,
) -> bool {
    // Writes in one recording execute in order. A different recording may run on another queue.
    readback.is_complete(completed)
        || (readback.claimed.is_none()
            && readback.queue.is_null()
            && readback.list == list
            && readback.generation == unsafe { &*owner.recordings.get() }.generation(list as usize))
}

pub(super) unsafe fn snapshot_submission(
    owner: &Capture,
    count: u32,
    lists: *const *mut c_void,
) -> *mut Submission {
    let lists = if count == 0 {
        &[]
    } else if lists.is_null() {
        return null_mut();
    } else {
        unsafe { std::slice::from_raw_parts(lists, count as usize) }
    };
    unsafe { &mut *owner.recordings.get() }
        .snapshot(lists)
        .map_or(null_mut(), allocate)
}

pub(super) fn claim_readback(submission: &Submission, readback: &mut Readback) -> bool {
    if readback.claimed.is_none()
        && readback.queue.is_null()
        && submission.generations.get(&(readback.list as usize)) == Some(&readback.generation)
    {
        readback.claimed = Some(submission.serial);
        return true;
    }
    false
}

pub(super) fn submit_readback(
    readback: &mut Readback,
    queue: *mut c_void,
    submission: u64,
) -> bool {
    if readback.claimed != Some(submission) {
        return false;
    }
    readback.claimed = None;
    readback.queue = queue;
    readback.submission = submission;
    true
}

pub(super) fn awaits_fence(readback: &Readback, queue: *mut c_void) -> bool {
    !queue.is_null() && readback.queue == queue && readback.fence_value == 0
}

pub(super) fn fence_readback(readback: &mut Readback, queue: *mut c_void, value: u64) -> bool {
    if value == 0 || !awaits_fence(readback, queue) {
        return false;
    }
    readback.fence_value = value;
    true
}

pub(super) unsafe fn apply_submission(owner: &Capture, submission: *mut Submission) -> u64 {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(submission) = unsafe { submission.as_mut() } {
        recordings.submit(submission)
    } else {
        // The queue ran after a refused snapshot; the old submitted states may be stale.
        recordings.submitted.clear();
        0
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_discard_submission(submission: *mut Submission) {
    if !submission.is_null() {
        drop(unsafe { Box::from_raw(submission) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::without_provenance_mut;

    #[test]
    fn split_and_aliasing_barriers_do_not_make_the_resource_readable() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let resource = without_provenance_mut(2);
        unsafe {
            rsf_rdr2_capture_initial_state(&owner, resource, 4);
            rsf_rdr2_capture_transition(&owner, list, resource, 1, 8, false);
            assert_eq!(rsf_rdr2_capture_resource_state(&owner, list, resource), 4);
            rsf_rdr2_capture_transition(&owner, list, resource, 0, 8, true);
            assert_eq!(
                rsf_rdr2_capture_resource_state(&owner, list, resource),
                UNKNOWN
            );
            rsf_rdr2_capture_transition(&owner, list, resource, u32::MAX, 8, false);
            assert_eq!(rsf_rdr2_capture_resource_state(&owner, list, resource), 8);
            rsf_rdr2_capture_alias(&owner, list, resource);
            assert_eq!(
                rsf_rdr2_capture_resource_state(&owner, list, resource),
                UNKNOWN
            );
        }
    }

    #[test]
    fn submission_keeps_the_recording_that_ran_when_the_list_is_reset() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let resource = without_provenance_mut(2);
        let other_list = without_provenance_mut(3);
        unsafe {
            rsf_rdr2_capture_reset_list(&owner, list, null_mut());
            rsf_rdr2_capture_transition(&owner, list, resource, 0, 8, false);
            let mut readback = Readback::default();
            stamp_copy(&owner, list, &mut readback);
            let batch = [list];
            let receipt = snapshot_submission(&owner, 1, batch.as_ptr());
            assert!(!receipt.is_null());
            rsf_rdr2_capture_reset_list(&owner, list, null_mut());
            rsf_rdr2_capture_transition(&owner, list, resource, 0, 16, false);
            let mut later = Readback::default();
            stamp_copy(&owner, list, &mut later);
            assert!(claim_readback(&*receipt, &mut readback));
            assert!(!claim_readback(&*receipt, &mut later));
            assert_eq!(apply_submission(&owner, receipt), 1);
            assert_eq!(
                rsf_rdr2_capture_resource_state(&owner, other_list, resource),
                8
            );
            assert_eq!(rsf_rdr2_capture_resource_state(&owner, list, resource), 16);
            rsf_rdr2_capture_discard_submission(receipt);
        }
    }

    #[test]
    fn the_submitted_readback_keeps_its_queue_until_a_fence_is_accepted() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let queue = without_provenance_mut(2);
        let other_queue = without_provenance_mut(3);
        let mut readback = Readback::default();
        unsafe {
            stamp_copy(&owner, list, &mut readback);
            assert!(reuse_readback(&owner, &readback, list, 0));
            let receipt = snapshot_submission(&owner, 1, [list].as_ptr());
            assert!(!receipt.is_null());
            assert!(claim_readback(&*receipt, &mut readback));
            assert!(!reuse_readback(&owner, &readback, list, 0));
            let serial = apply_submission(&owner, receipt);
            assert!(!submit_readback(&mut readback, other_queue, serial + 1));
            assert!(submit_readback(&mut readback, queue, serial));
            rsf_rdr2_capture_discard_submission(receipt);
        }
        assert!(awaits_fence(&readback, queue));
        assert!(!fence_readback(&mut readback, other_queue, 1));
        assert!(!readback.is_complete(1));
        // A future signal on the same queue covers the submitted copy, with or without its list.
        assert!(fence_readback(&mut readback, queue, 2));
        assert_eq!(readback.submission, 1);
        assert!(!readback.is_complete(u64::MAX));
        assert!(!readback.is_complete(1));
        assert!(readback.is_complete(2));
        assert!(unsafe { reuse_readback(&owner, &readback, list, 2) });
    }
}
