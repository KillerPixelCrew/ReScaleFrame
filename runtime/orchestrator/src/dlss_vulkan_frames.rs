// SPDX-License-Identifier: MIT
use crate::vulkan_fg_abi::{
    RSF_DLSS_VK_FRAME_SLOTS, rsf_dlss_vk_slot as Slot, rsf_dlss_vk_timeline as Timeline,
};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, rsf_frame_record};
use std::ffi::c_void;
use std::{ptr, slice};

const SLOTS: usize = RSF_DLSS_VK_FRAME_SLOTS as usize;

fn same_producer(a: &rsf_frame_record, b: &rsf_frame_record) -> bool {
    a.session_id == b.session_id
        && a.view_id == b.view_id
        && a.resource_generation == b.resource_generation
}

fn unsubmitted(slot: &Slot) -> bool {
    (slot.recorded_prepare != 0 && slot.submitted_prepare == 0)
        || (slot.recorded_hudless != 0 && slot.submitted_hudless == 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_unsubmitted(slot: &Slot) -> u32 {
    u32::from(unsubmitted(slot))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_can_prepare(slot: &Slot) -> u32 {
    u32::from(slot.completion_unknown == 0 && !unsubmitted(slot))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_prepare_begin(
    timeline: &mut Timeline,
    slot: &mut Slot,
    record: &rsf_frame_record,
    commands: *mut c_void,
) {
    let previous = &timeline.last_prepared;
    let reset = previous.frame_id.checked_add(1) != Some(record.frame_id)
        || !same_producer(previous, record);
    timeline.recording_sequence += 1;
    slot.recording_sequence = timeline.recording_sequence;
    slot.record = *record;
    if reset {
        slot.record.flags |= RSF_FRAME_FLAG_RESET;
    }
    slot.prepared = 0;
    slot.hudless = 0;
    slot.submitted_prepare = 0;
    slot.submitted_hudless = 0;
    slot.recorded_prepare = 0;
    slot.recorded_hudless = 0;
    slot.prepare_commands = commands;
    slot.hudless_commands = ptr::null_mut();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_prepare_accepted(timeline: &mut Timeline, slot: &Slot) {
    timeline.last_prepared = slot.record;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_hudless_begin(
    slot: &mut Slot,
    record: &rsf_frame_record,
    commands: *mut c_void,
) -> u32 {
    if slot.prepared == 0
        || slot.recorded_hudless != 0
        || slot.record.frame_id != record.frame_id
        || !same_producer(&slot.record, record)
    {
        return 0;
    }
    slot.hudless_commands = commands;
    1
}

fn recorded_for_submit(slot: &Slot, command: *mut c_void) -> bool {
    (slot.recorded_prepare != 0 && slot.prepare_commands == command && slot.submitted_prepare == 0)
        || (slot.recorded_hudless != 0
            && slot.hudless_commands == command
            && slot.submitted_hudless == 0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_vk_submitted(
    slots: *const *mut Slot,
    commands: *const *mut c_void,
    count: u32,
) -> u32 {
    if commands.is_null() {
        return 0;
    }
    let slots = unsafe { slice::from_raw_parts(slots, SLOTS) };
    let commands = unsafe { slice::from_raw_parts(commands, count as usize) };
    // A reset or re-recorded command list can reuse an old pointer. Only its
    // latest recording can prove a submission for these source leases.
    for &command in commands.iter().filter(|command| !command.is_null()) {
        let newest = slots
            .iter()
            .map(|&slot| unsafe { &*slot })
            .filter(|slot| recorded_for_submit(slot, command))
            .map(|slot| slot.recording_sequence)
            .max();
        let Some(newest) = newest else { continue };
        for &slot in slots {
            let slot = unsafe { &mut *slot };
            if slot.recording_sequence >= newest {
                continue;
            }
            if slot.prepare_commands == command && slot.submitted_prepare == 0 {
                slot.prepared = 0;
                slot.recorded_prepare = 0;
                slot.prepare_commands = ptr::null_mut();
            }
            if slot.hudless_commands == command && slot.submitted_hudless == 0 {
                slot.hudless = 0;
                slot.recorded_hudless = 0;
                slot.hudless_commands = ptr::null_mut();
            }
        }
    }
    let mut fences = 0;
    for (index, &slot) in slots.iter().enumerate() {
        let slot = unsafe { &mut *slot };
        let prepare = slot.recorded_prepare != 0
            && slot.submitted_prepare == 0
            && commands.contains(&slot.prepare_commands);
        let hudless = slot.recorded_hudless != 0
            && slot.submitted_hudless == 0
            && commands.contains(&slot.hudless_commands);
        slot.submitted_prepare |= u32::from(prepare);
        slot.submitted_hudless |= u32::from(hudless);
        if (prepare || hudless) && slot.completion_unknown == 0 {
            fences |= 1 << index;
        }
    }
    fences
}

fn matches_slot(slot: &Slot, source_frame: u64, identity_matches: u32) -> bool {
    identity_matches != 0 && source_frame != 0 && slot.record.frame_id == source_frame
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_present_candidate(
    timeline: &Timeline,
    slot: &Slot,
    source_frame: u64,
    identity_matches: u32,
    requested: u32,
) -> u32 {
    u32::from(
        matches_slot(slot, source_frame, identity_matches)
            && requested != 0
            && slot.recording_sequence > timeline.presented_sequence
            && slot.prepared != 0
            && slot.hudless != 0
            && slot.submitted_prepare != 0
            && slot.submitted_hudless != 0
            && slot.completion_unknown == 0,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_vk_presented(
    timeline: &mut Timeline,
    slot: &Slot,
    source_frame: u64,
    identity_matches: u32,
) {
    if matches_slot(slot, source_frame, identity_matches) {
        timeline.presented_sequence = timeline.presented_sequence.max(slot.recording_sequence);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_recording_in_the_same_slot_cannot_reset_accepted_history() {
        let mut timeline: Timeline = unsafe { std::mem::zeroed() };
        let mut slot: Slot = unsafe { std::mem::zeroed() };
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        let commands = ptr::without_provenance_mut(1);
        record.frame_id = 1;
        record.session_id = 1;
        record.resource_generation = 1;
        rsf_core_dlss_vk_prepare_begin(&mut timeline, &mut slot, &record, commands);
        rsf_core_dlss_vk_prepare_accepted(&mut timeline, &slot);
        record.frame_id = 1 + SLOTS as u64;
        record.resource_generation = 2;
        rsf_core_dlss_vk_prepare_begin(&mut timeline, &mut slot, &record, commands);
        assert_ne!(slot.record.flags & RSF_FRAME_FLAG_RESET, 0);

        record.frame_id = 2;
        record.resource_generation = 1;
        rsf_core_dlss_vk_prepare_begin(&mut timeline, &mut slot, &record, commands);
        assert_eq!(slot.record.flags & RSF_FRAME_FLAG_RESET, 0);
        assert_eq!(timeline.last_prepared.frame_id, 1);
    }

    #[test]
    fn hudless_recording_cannot_overwrite_an_in_flight_copy() {
        let mut slot: Slot = unsafe { std::mem::zeroed() };
        slot.prepared = 1;
        slot.record.frame_id = 1;
        slot.record.session_id = 1;
        let record = slot.record;
        let first = ptr::without_provenance_mut(1);
        assert_eq!(rsf_core_dlss_vk_hudless_begin(&mut slot, &record, first), 1);
        slot.recorded_hudless = 1;
        let duplicate = ptr::without_provenance_mut(2);
        assert_eq!(
            rsf_core_dlss_vk_hudless_begin(&mut slot, &record, duplicate),
            0
        );
        assert_eq!(slot.hudless_commands, first);
    }
}
