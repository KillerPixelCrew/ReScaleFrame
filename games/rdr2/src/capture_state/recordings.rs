// SPDX-License-Identifier: MIT
//! Resource and generation facts for the native capture mutex. No map holds a COM reference.
use super::{Capture, allocate};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

const UNKNOWN: u32 = u32::MAX;
type States = HashMap<usize, u32>;

#[derive(Default)]
struct Recording {
    generation: u64,
    states: States,
}

#[derive(Default)]
pub(super) struct Recordings {
    lists: HashMap<usize, Recording>,
    submitted: States,
    submission_serial: u64,
}

struct Submission {
    serial: u64,
    generations: HashMap<usize, u64>,
    states: States,
}

#[repr(C)]
#[derive(Default)]
struct Readback {
    generation: u64,
    submission: u64,
    fence_value: u64,
    list: *mut c_void,
    queue: *mut c_void,
    claimed: u32,
}

const _: () = assert!(size_of::<Readback>() == 48 && std::mem::offset_of!(Readback, list) == 24);

fn try_entry<V: Default>(map: &mut HashMap<usize, V>, key: usize) -> Option<&mut V> {
    if !map.contains_key(&key) && map.try_reserve(1).is_err() {
        return None;
    }
    Some(map.entry(key).or_default())
}

impl Recordings {
    fn generation(&self, list: usize) -> u64 {
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
        self.submission_serial = self.submission_serial.wrapping_add(1);
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

// All of these calls run under g->mutex. Resource and command-list addresses are identities.
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
unsafe extern "C" fn rsf_rdr2_capture_reset_list(owner: &Capture, list: *mut c_void) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    if let Some(recording) = try_entry(&mut recordings.lists, list as usize) {
        recording.generation = recording.generation.wrapping_add(1);
        recording.states.clear();
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_list_generation(owner: &Capture, list: *mut c_void) -> u64 {
    unsafe { &*owner.recordings.get() }.generation(list as usize)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_stamp_copy(
    owner: &Capture,
    list: *mut c_void,
    readback: &mut Readback,
) {
    let recordings = unsafe { &mut *owner.recordings.get() };
    let generation =
        try_entry(&mut recordings.lists, list as usize).map_or(0, |recording| recording.generation);
    *readback = Readback {
        generation,
        list,
        ..Readback::default()
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_reuse_readback(
    owner: &Capture,
    readback: &Readback,
    list: *mut c_void,
    completed: u64,
) -> bool {
    // Writes in one recording execute in order. A different recording may run on another queue.
    rsf_rdr2_capture_readback_complete(readback, completed)
        || (readback.claimed == 0
            && readback.queue.is_null()
            && readback.list == list
            && readback.generation == unsafe { &*owner.recordings.get() }.generation(list as usize))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_readback_complete(readback: &Readback, completed: u64) -> bool {
    readback.fence_value != 0 && completed != u64::MAX && completed >= readback.fence_value
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_before_submit(
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

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_claim_readback(
    submission: &Submission,
    readback: &mut Readback,
) -> bool {
    if readback.claimed != 0
        || !readback.queue.is_null()
        || submission.generations.get(&(readback.list as usize)) != Some(&readback.generation)
    {
        return false;
    }
    readback.claimed = 1;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_submit_readback(
    readback: &mut Readback,
    queue: *mut c_void,
    submission: u64,
) {
    readback.claimed = 0;
    readback.queue = queue;
    readback.submission = submission;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_readback_awaits_fence(
    readback: &Readback,
    queue: *mut c_void,
) -> bool {
    !queue.is_null() && readback.queue == queue && readback.fence_value == 0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_fence_readback(
    readback: &mut Readback,
    queue: *mut c_void,
    value: u64,
) -> bool {
    if value == 0 || !rsf_rdr2_capture_readback_awaits_fence(readback, queue) {
        return false;
    }
    readback.fence_value = value;
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_after_submit(
    owner: &Capture,
    submission: *mut Submission,
) -> u64 {
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
            rsf_rdr2_capture_reset_list(&owner, list);
            rsf_rdr2_capture_transition(&owner, list, resource, 0, 8, false);
            let mut readback = Readback::default();
            rsf_rdr2_capture_stamp_copy(&owner, list, &mut readback);
            let batch = [list];
            let receipt = rsf_rdr2_capture_before_submit(&owner, 1, batch.as_ptr());
            assert!(!receipt.is_null());
            rsf_rdr2_capture_reset_list(&owner, list);
            rsf_rdr2_capture_transition(&owner, list, resource, 0, 16, false);
            let mut later = Readback::default();
            rsf_rdr2_capture_stamp_copy(&owner, list, &mut later);
            assert!(rsf_rdr2_capture_claim_readback(&*receipt, &mut readback));
            assert!(!rsf_rdr2_capture_claim_readback(&*receipt, &mut later));
            assert_eq!(rsf_rdr2_capture_after_submit(&owner, receipt), 1);
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
            rsf_rdr2_capture_stamp_copy(&owner, list, &mut readback);
            assert!(rsf_rdr2_capture_reuse_readback(&owner, &readback, list, 0));
            let receipt = rsf_rdr2_capture_before_submit(&owner, 1, [list].as_ptr());
            assert!(!receipt.is_null());
            assert!(rsf_rdr2_capture_claim_readback(&*receipt, &mut readback));
            assert!(!rsf_rdr2_capture_reuse_readback(&owner, &readback, list, 0));
            let serial = rsf_rdr2_capture_after_submit(&owner, receipt);
            rsf_rdr2_capture_submit_readback(&mut readback, queue, serial);
            rsf_rdr2_capture_discard_submission(receipt);
        }
        assert!(rsf_rdr2_capture_readback_awaits_fence(&readback, queue));
        assert!(!rsf_rdr2_capture_fence_readback(
            &mut readback,
            other_queue,
            1
        ));
        assert!(!rsf_rdr2_capture_readback_complete(&readback, 1));
        // A future signal on the same queue covers the submitted copy, with or without its list.
        assert!(rsf_rdr2_capture_fence_readback(&mut readback, queue, 2));
        assert_eq!(readback.submission, 1);
        assert!(!rsf_rdr2_capture_readback_complete(&readback, u64::MAX));
        assert!(!rsf_rdr2_capture_readback_complete(&readback, 1));
        assert!(rsf_rdr2_capture_readback_complete(&readback, 2));
        assert!(unsafe { rsf_rdr2_capture_reuse_readback(&owner, &readback, list, 2) });
    }
}
