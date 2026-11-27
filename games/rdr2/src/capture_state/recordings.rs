// SPDX-License-Identifier: MIT
//! Command-list observations and copied draw facts ordered by the native capture mutex.
use super::{Capture, MAX_DRAW_METADATA, try_entry};
#[cfg(windows)]
use rsf_game_sys::checksum::crc32_ieee;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

const UNKNOWN: u32 = u32::MAX;
type States = HashMap<usize, u32>;

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

pub(super) struct DrawBinding {
    pub(super) target: *mut c_void,
    pub(super) pso: u64,
    pub(super) pipeline: Pipeline,
    pub(super) pipeline_known: bool,
}

#[derive(Clone, Copy, Default)]
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
    pub(super) pipeline_known: bool,
}

#[derive(Default)]
struct Recording {
    generation: u64,
    states: States,
    wildcard_alias: bool,
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
    wildcard_alias: bool,
}

#[derive(Default)]
pub(super) struct Readback {
    pub(super) generation: u64,
    pub(super) submission: u64,
    pub(super) fence_value: u64,
    pub(super) list: *mut c_void,
    pub(super) queue: *mut c_void,
    claimed: Option<u64>,
    pub(super) untracked: bool,
    pub(super) discarded: bool,
}

impl Readback {
    pub(super) fn is_complete(&self, completed: u64) -> bool {
        self.fence_value != 0 && completed != u64::MAX && completed >= self.fence_value
    }

    pub(super) fn discarded_by_reset(&self, list: *mut c_void, generation: u64) -> bool {
        self.list == list
            && self.generation == generation
            && self.claimed.is_none()
            && self.submission == 0
            && !self.untracked
    }
}

impl Recordings {
    pub(super) fn generation(&self, list: usize) -> u64 {
        self.lists
            .get(&list)
            .map_or(0, |recording| recording.generation)
    }

    fn state(&self, list: usize, resource: usize) -> u32 {
        if let Some(recording) = self.lists.get(&list) {
            if let Some(&state) = recording.states.get(&resource) {
                return state;
            }
            if recording.wildcard_alias {
                return UNKNOWN;
            }
        }
        self.submitted.get(&resource).copied().unwrap_or(UNKNOWN)
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
            wildcard_alias: false,
        };
        submission.generations.try_reserve(lists.len()).ok()?;
        for &list in lists {
            let list = list as usize;
            if let Some(recording) = self.lists.get(&list) {
                submission.generations.insert(list, recording.generation);
                if recording.wildcard_alias {
                    submission.states.clear();
                    submission.wildcard_alias = true;
                }
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
        if submission.wildcard_alias {
            self.submitted.clear();
        }
        if self.submitted.try_reserve(submission.states.len()).is_ok() {
            self.submitted.extend(submission.states.drain());
        } else {
            // Do not serve stale global facts after a submission we could not track.
            self.submitted.clear();
        }
        submission.serial
    }
}

// The descriptor lends the exact shader bytes for this call. Hashing runs before the capture
// mutex is taken.
#[cfg(windows)]
pub(super) unsafe fn hash_shader(bytes: *const u8, length: usize) -> Option<u32> {
    if length == 0 {
        return Some(0);
    }
    if bytes.is_null() || length > isize::MAX as usize {
        return None;
    }
    Some(crc32_ieee(unsafe {
        std::slice::from_raw_parts(bytes, length)
    }))
}

// All of these calls run under g->mutex. Resource and command-list addresses are identities.
// The observation maps do not own COM references.
pub(super) unsafe fn record_pipeline(owner: &Capture, pso: *mut c_void, facts: Option<&Pipeline>) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    let key = pso as usize;
    let Some(facts) = facts else {
        // A successful callback with an unrecognized descriptor supersedes an old address.
        recordings.pipelines.remove(&key);
        return;
    };
    if let Some(pipeline) = try_entry(&mut recordings.pipelines, key) {
        *pipeline = *facts;
    }
}

pub(super) unsafe fn record_rtv(owner: &Capture, handle: usize, resource: *mut c_void) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if resource.is_null() {
        recordings.rtvs.remove(&handle);
    } else if let Some(target) = try_entry(&mut recordings.rtvs, handle) {
        *target = resource;
    }
}

pub(super) unsafe fn bind_rtv(owner: &Capture, list: *mut c_void, handle: Option<usize>) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    // Resolve the first view at bind time. A later descriptor write does not retarget the list.
    let target = handle
        .and_then(|handle| recordings.rtvs.get(&handle))
        .copied()
        .unwrap_or(null_mut());
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.target = target;
    }
}

pub(super) unsafe fn bind_pipeline(owner: &Capture, list: *mut c_void, pso: *mut c_void) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.pso = pso as usize;
    }
}

pub(super) unsafe fn draw_binding(owner: &Capture, list: *mut c_void) -> Option<DrawBinding> {
    let recordings = unsafe { &*owner.recordings.get() };
    let recording = recordings.lists.get(&(list as usize))?;
    if recording.target.is_null() {
        return None;
    }
    let pipeline = recordings.pipelines.get(&recording.pso).copied();
    Some(DrawBinding {
        target: recording.target,
        pso: recording.pso as u64,
        pipeline: pipeline.unwrap_or_default(),
        pipeline_known: pipeline.is_some(),
    })
}

pub(super) unsafe fn record_draw(owner: &Capture, draw: &Draw) -> bool {
    let recordings = unsafe { &mut *owner.recordings.get() };
    let draws = &mut recordings.draws;
    if draws.len() >= MAX_DRAW_METADATA || draws.try_reserve(1).is_err() {
        return false;
    }
    draws.push(*draw);
    true
}

pub(super) unsafe fn initial_state(owner: &Capture, resource: *mut c_void, state: u32) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(value) = try_entry(&mut recordings.submitted, resource as usize) {
        *value = state;
    }
}

pub(super) unsafe fn transition(
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

pub(super) unsafe fn alias(owner: &Capture, list: *mut c_void, resource: *mut c_void) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if !resource.is_null() {
        recordings.record(list as usize, resource as usize, UNKNOWN);
    } else if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.states.clear();
        recording.wildcard_alias = true;
    } else {
        recordings.submitted.clear();
    }
}

pub(super) unsafe fn reset_list(owner: &Capture, list: *mut c_void, pso: *mut c_void) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        unsafe { super::shots::discard_reset(owner, list, recording.generation) };
        recording.generation = recording.generation.wrapping_add(1);
        recording.states.clear();
        recording.wildcard_alias = false;
        recording.target = null_mut();
        recording.pso = pso as usize;
    }
}

pub(super) unsafe fn resource_state(
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
        || (!readback.untracked
            && !readback.discarded
            && readback.claimed.is_none()
            && readback.queue.is_null()
            && readback.list == list
            && readback.generation == unsafe { &*owner.recordings.get() }.generation(list as usize))
}

pub(super) unsafe fn snapshot_submission(
    owner: &Capture,
    lists: &[*mut c_void],
) -> Option<Submission> {
    unsafe { &mut *owner.recordings.get() }.snapshot(lists)
}

pub(super) fn claim_readback(submission: &Submission, readback: &mut Readback) -> bool {
    if !readback.untracked
        && !readback.discarded
        && readback.claimed.is_none()
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

pub(super) unsafe fn apply_submission(owner: &Capture, submission: Option<&mut Submission>) -> u64 {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(submission) = submission {
        recordings.submit(submission)
    } else {
        // The queue ran after a refused snapshot; the old submitted states may be stale.
        recordings.submitted.clear();
        0
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
            initial_state(&owner, resource, 4);
            transition(&owner, list, resource, 1, 8, false);
            assert_eq!(resource_state(&owner, list, resource), 4);
            transition(&owner, list, resource, 0, 8, true);
            assert_eq!(resource_state(&owner, list, resource), UNKNOWN);
            transition(&owner, list, resource, u32::MAX, 8, false);
            assert_eq!(resource_state(&owner, list, resource), 8);
            alias(&owner, list, resource);
            assert_eq!(resource_state(&owner, list, resource), UNKNOWN);
        }
    }

    #[test]
    fn wildcard_alias_invalidates_earlier_lists_and_preserves_later_transitions() {
        let owner = Capture::new();
        let earlier = without_provenance_mut(1);
        let aliased = without_provenance_mut(2);
        let later = without_provenance_mut(3);
        let unrelated_list = without_provenance_mut(4);
        let a = without_provenance_mut(5);
        let b = without_provenance_mut(6);
        let c = without_provenance_mut(7);
        unsafe {
            initial_state(&owner, a, 4);
            transition(&owner, earlier, b, 0, 8, false);
            transition(&owner, aliased, a, 0, 8, false);
            alias(&owner, aliased, null_mut());
            assert_eq!(resource_state(&owner, aliased, a), UNKNOWN);
            transition(&owner, aliased, c, 0, 16, false);
            transition(&owner, later, b, 0, 32, false);
            let mut submission = snapshot_submission(&owner, &[earlier, aliased, later]);
            assert!(submission.is_some());
            apply_submission(&owner, submission.as_mut());
            assert_eq!(resource_state(&owner, unrelated_list, a), UNKNOWN);
            assert_eq!(resource_state(&owner, unrelated_list, b), 32);
            assert_eq!(resource_state(&owner, unrelated_list, c), 16);
        }
    }

    #[test]
    fn bound_pipeline_keeps_the_pso_identity_after_facts_are_withdrawn() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let resource = without_provenance_mut(2);
        let pso = without_provenance_mut(3);
        unsafe {
            record_rtv(&owner, 4, resource);
            bind_rtv(&owner, list, Some(4));
            bind_pipeline(&owner, list, pso);
            record_pipeline(&owner, pso, Some(&Pipeline::default()));
            assert!(draw_binding(&owner, list).unwrap().pipeline_known);
            record_pipeline(&owner, pso, None);
            let binding = draw_binding(&owner, list).unwrap();
            assert!(!binding.pipeline_known);
            assert_eq!(binding.pso, 3);
            assert_eq!(binding.target, resource);
        }
    }

    #[test]
    fn submission_keeps_the_recording_that_ran_when_the_list_is_reset() {
        let owner = Capture::new();
        let list = without_provenance_mut(1);
        let resource = without_provenance_mut(2);
        let other_list = without_provenance_mut(3);
        unsafe {
            reset_list(&owner, list, null_mut());
            transition(&owner, list, resource, 0, 8, false);
            let mut readback = Readback::default();
            stamp_copy(&owner, list, &mut readback);
            let mut receipt = snapshot_submission(&owner, &[list]).unwrap();
            reset_list(&owner, list, null_mut());
            transition(&owner, list, resource, 0, 16, false);
            let mut later = Readback::default();
            stamp_copy(&owner, list, &mut later);
            assert!(claim_readback(&receipt, &mut readback));
            assert!(!claim_readback(&receipt, &mut later));
            assert_eq!(apply_submission(&owner, Some(&mut receipt)), 1);
            assert_eq!(resource_state(&owner, other_list, resource), 8);
            assert_eq!(resource_state(&owner, list, resource), 16);
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
            let mut receipt = snapshot_submission(&owner, &[list]).unwrap();
            assert!(claim_readback(&receipt, &mut readback));
            assert!(!reuse_readback(&owner, &readback, list, 0));
            let serial = apply_submission(&owner, Some(&mut receipt));
            assert!(!submit_readback(&mut readback, other_queue, serial + 1));
            assert!(submit_readback(&mut readback, queue, serial));
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
