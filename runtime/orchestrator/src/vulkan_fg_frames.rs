// SPDX-License-Identifier: MIT

use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_RESET,
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::ffi::c_void;
use std::mem::offset_of;
use std::ptr;

/// Recording slots per Vulkan FG owner.
pub(crate) const FRAME_SLOTS: u32 = 6;
const SLOTS: usize = FRAME_SLOTS as usize;

/// One recording slot: the leased source record, its prepare and HUD-less command buffers and
/// what was recorded, submitted and is still pending for them.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct FrameSlot {
    pub(crate) record: rsf_frame_record,
    pub(crate) recording_sequence: u64,
    pub(crate) prepare_commands: *mut c_void,
    pub(crate) hudless_commands: *mut c_void,
    pub(crate) guides: u32,
    pub(crate) prepared: u32,
    pub(crate) hudless: u32,
    pub(crate) submitted_prepare: u32,
    pub(crate) submitted_hudless: u32,
    pub(crate) recorded_prepare: u32,
    pub(crate) recorded_hudless: u32,
    pub(crate) source_pending: u32,
    pub(crate) consumer_pending: u32,
    pub(crate) completion_unknown: u32,
}
use self::FrameSlot as Slot;

/// The owner's last prepared source and its recording and presented sequence numbers.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Timeline {
    pub(crate) last_prepared: rsf_frame_record,
    pub(crate) recording_sequence: u64,
    pub(crate) presented_sequence: u64,
}

/// What one submission notification did: fences to arm and recordings it discarded.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SubmitPlan {
    pub(crate) fences: u32,
    pub(crate) discarded_prepare: u32,
    pub(crate) discarded_hudless: u32,
}

/// The recording slot of a frame ID, shared by every Vulkan FG provider.
pub(crate) fn slot_index(frame: u64) -> usize {
    (frame % SLOTS as u64) as usize
}
const FG_VETO: u32 = RSF_FRAME_FLAG_NO_FG | RSF_FRAME_FLAG_AMBIGUOUS_ID;

impl Slot {
    pub(super) fn capture_group(&self, hudless: bool) -> u64 {
        (self.recording_sequence << 1) | u64::from(hudless)
    }
}

pub(super) fn valid_source_record(
    record: &rsf_frame_record,
    width: u32,
    height: u32,
    infinite_depth: bool,
) -> bool {
    let camera = &record.camera;
    let positive = |value: f32| value.is_finite() && value > 0.0;
    record.abi_version == RSF_GAME_FRAME_ABI_VERSION
        && record.session_id != 0
        && record.frame_id != 0
        && camera.struct_size as usize >= size_of_val(camera)
        && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
        && (1..=width).contains(&record.render_width)
        && (1..=height).contains(&record.render_height)
        && record.output_width == width
        && record.output_height == height
        && camera.render_width == record.render_width
        && camera.render_height == record.render_height
        && camera.output_width == width
        && camera.output_height == height
        && positive(record.frame_time_ms)
        && positive(camera.near_plane)
        && camera.far_plane.is_finite()
        && camera.far_plane >= 0.0
        && (infinite_depth || camera.far_plane > camera.near_plane)
        && positive(camera.vertical_fov_radians)
        && camera.vertical_fov_radians <= std::f32::consts::PI
        && camera
            .jitter_pixels
            .iter()
            .chain(&camera.view_to_clip)
            .chain(&camera.clip_to_view)
            .chain(&camera.clip_to_previous_clip)
            .chain(&camera.view_to_world)
            .all(|value| value.is_finite())
}

// The count-only call may receive uninitialized output storage from Vulkan.
pub(super) unsafe fn bounded_swapchain_images(
    count: *mut u32,
    output: *mut u64,
    mut enumerate: impl FnMut(*mut u32, *mut u64) -> i32,
) -> i32 {
    const SUCCESS: i32 = 0;
    const INCOMPLETE: i32 = 5;
    const INITIALIZATION_FAILED: i32 = -3;
    if output.is_null() {
        return enumerate(count, output);
    }
    // A proxy can leave an oversized capacity unchanged after writing fewer images.
    let capacity = unsafe { count.read() };
    let mut available = 0;
    let queried = enumerate(&mut available, ptr::null_mut());
    if queried != SUCCESS {
        unsafe { count.write(0) };
        return queried;
    }
    let limit = capacity.min(available);
    let mut written = limit;
    let filled = enumerate(&mut written, output);
    if written > limit {
        unsafe { count.write(0) };
        return INITIALIZATION_FAILED;
    }
    unsafe { count.write(written) };
    if filled == SUCCESS && capacity < available {
        INCOMPLETE
    } else {
        filled
    }
}

pub(super) struct PresentIdentity {
    frame: u64,
    session: u64,
    view: u32,
    generation: u32,
    veto: bool,
}

pub(super) enum PresentSource {
    Implicit,
    Explicit(Option<PresentIdentity>),
}

impl PresentSource {
    pub(super) unsafe fn valid_prefix(source: *const rsf_frame_record) -> bool {
        source.is_null()
            || (source.is_aligned()
                && (unsafe { source.cast::<u32>().read() } as usize)
                    >= offset_of!(rsf_frame_record, phase)
                && unsafe { (*source).abi_version } == RSF_GAME_FRAME_ABI_VERSION)
    }

    pub(super) unsafe fn read(source: *const rsf_frame_record, explicit: u32) -> Option<Self> {
        if explicit == 0 {
            return Some(Self::Implicit);
        }
        if source.is_null() {
            return Some(Self::Explicit(None));
        }
        // The identity prefix may be the entire allocation.
        if !unsafe { Self::valid_prefix(source) } {
            return None;
        }
        let size = unsafe { source.cast::<u32>().read() } as usize;
        // Flags can veto FG after prepare. The shorter identity prefix
        // remains valid and has no flags to read.
        let flags_end = offset_of!(rsf_frame_record, flags) + size_of::<u32>();
        let veto = size >= flags_end && unsafe { (*source).flags } & FG_VETO != 0;
        Some(Self::Explicit(Some(unsafe {
            PresentIdentity {
                frame: (*source).frame_id,
                session: (*source).session_id,
                view: (*source).view_id,
                generation: (*source).resource_generation,
                veto,
            }
        })))
    }

    pub(super) fn allows_fg(&self) -> bool {
        match self {
            Self::Implicit => true,
            Self::Explicit(identity) => identity.as_ref().is_some_and(|identity| !identity.veto),
        }
    }

    pub(super) fn frame(&self, latest: u64) -> u64 {
        match self {
            Self::Implicit => latest,
            Self::Explicit(identity) => identity.as_ref().map_or(0, |identity| identity.frame),
        }
    }

    pub(super) fn matches(&self, record: &rsf_frame_record) -> bool {
        match self {
            Self::Implicit => true,
            Self::Explicit(identity) => identity.as_ref().is_some_and(|identity| {
                identity.frame != 0
                    && identity.session != 0
                    && identity.frame == record.frame_id
                    && identity.session == record.session_id
                    && identity.view == record.view_id
                    && identity.generation == record.resource_generation
                    && record.flags & FG_VETO == 0
            }),
        }
    }
}

fn same_producer(a: &rsf_frame_record, b: &rsf_frame_record) -> bool {
    a.session_id == b.session_id
        && a.view_id == b.view_id
        && a.resource_generation == b.resource_generation
}

fn unsubmitted(slot: &Slot) -> bool {
    (slot.recorded_prepare != 0 && slot.submitted_prepare == 0)
        || (slot.recorded_hudless != 0 && slot.submitted_hudless == 0)
}

pub(super) extern "C" fn rsf_core_fgvk_unsubmitted(slot: &Slot) -> u32 {
    u32::from(unsubmitted(slot))
}

pub(super) extern "C" fn rsf_core_fgvk_can_prepare(slot: &Slot) -> u32 {
    u32::from(slot.completion_unknown == 0 && !unsubmitted(slot))
}

pub(super) extern "C" fn rsf_core_fgvk_prepare_begin(
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
    slot.guides = 0;
    slot.prepared = 0;
    slot.hudless = 0;
    slot.submitted_prepare = 0;
    slot.submitted_hudless = 0;
    slot.recorded_prepare = 0;
    slot.recorded_hudless = 0;
    slot.prepare_commands = commands;
    slot.hudless_commands = ptr::null_mut();
}

pub(super) extern "C" fn rsf_core_fgvk_prepare_accepted(timeline: &mut Timeline, slot: &mut Slot) {
    slot.guides = 1;
    timeline.last_prepared = slot.record;
}

pub(super) extern "C" fn rsf_core_fgvk_hudless_begin(
    slot: &mut Slot,
    record: &rsf_frame_record,
    commands: *mut c_void,
) -> u32 {
    if slot.guides == 0
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

pub(super) fn submission_plan(
    mut slots: [&mut Slot; SLOTS],
    commands: &[*mut c_void],
) -> SubmitPlan {
    let mut out = SubmitPlan {
        fences: 0,
        discarded_prepare: 0,
        discarded_hudless: 0,
    };
    // A reset or re-recorded command list can reuse an old pointer. Only its
    // latest recording can prove a submission for these source leases.
    for &command in commands.iter().filter(|command| !command.is_null()) {
        let newest = slots
            .iter()
            .filter(|slot| recorded_for_submit(slot, command))
            .map(|slot| slot.recording_sequence)
            .max();
        let Some(newest) = newest else { continue };
        for (index, slot) in slots.iter_mut().enumerate() {
            if slot.recording_sequence >= newest {
                continue;
            }
            if slot.prepare_commands == command && slot.submitted_prepare == 0 {
                if slot.recorded_prepare != 0 {
                    out.discarded_prepare |= 1 << index;
                }
                slot.guides = 0;
                slot.prepared = 0;
                slot.recorded_prepare = 0;
                slot.prepare_commands = ptr::null_mut();
            }
            if slot.hudless_commands == command && slot.submitted_hudless == 0 {
                if slot.recorded_hudless != 0 {
                    out.discarded_hudless |= 1 << index;
                }
                slot.hudless = 0;
                slot.recorded_hudless = 0;
                slot.hudless_commands = ptr::null_mut();
            }
        }
    }
    for (index, slot) in slots.iter_mut().enumerate() {
        let prepare = slot.recorded_prepare != 0
            && slot.submitted_prepare == 0
            && !slot.prepare_commands.is_null()
            && commands.contains(&slot.prepare_commands);
        let hudless = slot.recorded_hudless != 0
            && slot.submitted_hudless == 0
            && !slot.hudless_commands.is_null()
            && commands.contains(&slot.hudless_commands);
        slot.submitted_prepare |= u32::from(prepare);
        slot.submitted_hudless |= u32::from(hudless);
        if (prepare || hudless) && slot.completion_unknown == 0 {
            out.fences |= 1 << index;
        }
    }
    out
}

// The former C entry over submission_plan; only the unit tests below still drive it.
#[cfg(test)]
unsafe extern "C" fn rsf_core_fgvk_submitted(
    slots: *const *mut Slot,
    commands: *const *mut c_void,
    count: u32,
    out: &mut SubmitPlan,
) {
    if commands.is_null() {
        *out = SubmitPlan {
            fences: 0,
            discarded_prepare: 0,
            discarded_hudless: 0,
        };
        return;
    }
    let slots = unsafe { slots.cast::<[*mut Slot; SLOTS]>().read() };
    let commands = unsafe { std::slice::from_raw_parts(commands, count as usize) };
    // The native caller supplies six distinct slots tied to its resource ring.
    *out = submission_plan(slots.map(|slot| unsafe { &mut *slot }), commands);
}

fn matches_slot(slot: &Slot, source_frame: u64, identity_matches: u32) -> bool {
    identity_matches != 0 && source_frame != 0 && slot.record.frame_id == source_frame
}

pub(super) extern "C" fn rsf_core_fgvk_present_candidate(
    timeline: &Timeline,
    slot: &Slot,
    source_frame: u64,
    identity_matches: u32,
    requested: u32,
) -> u32 {
    u32::from(
        matches_slot(slot, source_frame, identity_matches)
            && requested != 0
            && slot.record.flags & FG_VETO == 0
            && slot.recording_sequence > timeline.presented_sequence
            && slot.prepared != 0
            && slot.hudless != 0
            && slot.submitted_prepare != 0
            && slot.submitted_hudless != 0
            && slot.completion_unknown == 0,
    )
}

pub(super) extern "C" fn rsf_core_fgvk_presented(
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
    fn swapchain_image_enumeration_bounds_the_fill_and_returned_count() {
        let mut images = [0; 4];
        let output = images.as_mut_ptr();
        for (capacity, output, query, fill_count, expected_result, expected_count) in [
            (0, ptr::null_mut(), 0, None, 0, 2),
            (4, output, -4, None, -4, 0),
            (1, output, 0, Some(1), 5, 1),
            (4, output, 0, Some(2), 0, 2),
            (4, output, 0, Some(3), -3, 0),
        ] {
            let mut count = capacity;
            let mut filled = false;
            let result = unsafe {
                bounded_swapchain_images(&mut count, output, |count, images| {
                    if images.is_null() {
                        count.write(2);
                        query
                    } else {
                        assert_eq!(count.read(), capacity.min(2));
                        count.write(fill_count.unwrap());
                        filled = true;
                        0
                    }
                })
            };
            assert_eq!(
                (result, count, filled),
                (expected_result, expected_count, fill_count.is_some())
            );
        }
    }

    #[test]
    fn count_only_supports_uninitialized_output_storage() {
        let mut count = std::mem::MaybeUninit::<u32>::uninit();
        let result = unsafe {
            bounded_swapchain_images(count.as_mut_ptr(), ptr::null_mut(), |count, images| {
                assert!(images.is_null());
                count.write(3);
                0
            })
        };
        assert_eq!(result, 0);
        assert_eq!(unsafe { count.assume_init() }, 3);
    }

    #[test]
    fn explicit_present_uses_optional_flags_without_requiring_a_full_record() {
        #[repr(C)]
        struct Prefix {
            size: u32,
            version: u32,
            frame: u64,
            session: u64,
            view: u32,
            generation: u32,
        }
        assert_eq!(size_of::<Prefix>(), offset_of!(rsf_frame_record, phase));
        let prefix = Prefix {
            size: size_of::<Prefix>() as u32,
            version: RSF_GAME_FRAME_ABI_VERSION,
            frame: 1,
            session: 2,
            view: 3,
            generation: 4,
        };
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = prefix.version;
        record.frame_id = prefix.frame;
        record.session_id = prefix.session;
        record.view_id = prefix.view;
        record.resource_generation = prefix.generation;
        let identity = unsafe { PresentSource::read(ptr::from_ref(&prefix).cast(), 1) }.unwrap();
        assert!(identity.matches(&record) && identity.allows_fg());

        for flag in [RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_AMBIGUOUS_ID] {
            let late = rsf_frame_record {
                flags: flag,
                ..record
            };
            let source = unsafe { PresentSource::read(&late, 1) }.unwrap();
            assert!(source.matches(&record) && !source.allows_fg());
        }
    }

    #[test]
    fn stored_veto_applies_to_implicit_and_explicit_present() {
        let timeline: Timeline = unsafe { std::mem::zeroed() };
        let mut slot: Slot = unsafe { std::mem::zeroed() };
        slot.record.struct_size = size_of::<rsf_frame_record>() as u32;
        slot.record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        slot.record.frame_id = 1;
        slot.record.session_id = 2;
        slot.recording_sequence = 1;
        slot.prepared = 1;
        slot.hudless = 1;
        slot.submitted_prepare = 1;
        slot.submitted_hudless = 1;
        let clean = slot.record;
        for explicit in [0, 1] {
            let source = unsafe { PresentSource::read(&clean, explicit) }.unwrap();
            for flags in [0, RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_AMBIGUOUS_ID] {
                slot.record.flags = flags;
                assert_eq!(
                    rsf_core_fgvk_present_candidate(
                        &timeline,
                        &slot,
                        source.frame(1),
                        u32::from(source.matches(&slot.record)),
                        u32::from(source.allows_fg()),
                    ),
                    u32::from(flags == 0),
                );
            }
        }
    }

    #[test]
    fn rejected_recording_in_the_same_slot_cannot_reset_accepted_history() {
        let mut timeline: Timeline = unsafe { std::mem::zeroed() };
        let mut slot: Slot = unsafe { std::mem::zeroed() };
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        let commands = ptr::without_provenance_mut(1);
        record.frame_id = 1;
        record.session_id = 1;
        record.resource_generation = 1;
        rsf_core_fgvk_prepare_begin(&mut timeline, &mut slot, &record, commands);
        rsf_core_fgvk_prepare_accepted(&mut timeline, &mut slot);
        record.frame_id = 1 + SLOTS as u64;
        record.resource_generation = 2;
        rsf_core_fgvk_prepare_begin(&mut timeline, &mut slot, &record, commands);
        assert_ne!(slot.record.flags & RSF_FRAME_FLAG_RESET, 0);

        record.frame_id = 2;
        record.resource_generation = 1;
        rsf_core_fgvk_prepare_begin(&mut timeline, &mut slot, &record, commands);
        assert_eq!(slot.record.flags & RSF_FRAME_FLAG_RESET, 0);
        assert_eq!(timeline.last_prepared.frame_id, 1);
    }

    #[test]
    fn hudless_recording_cannot_overwrite_an_in_flight_copy() {
        let mut slot: Slot = unsafe { std::mem::zeroed() };
        // A capture-only source has guides even when interpolation is Off.
        slot.guides = 1;
        slot.record.frame_id = 1;
        slot.record.session_id = 1;
        let record = slot.record;
        let first = ptr::without_provenance_mut(1);
        assert_eq!(rsf_core_fgvk_hudless_begin(&mut slot, &record, first), 1);
        slot.recorded_hudless = 1;
        let duplicate = ptr::without_provenance_mut(2);
        assert_eq!(
            rsf_core_fgvk_hudless_begin(&mut slot, &record, duplicate),
            0
        );
        assert_eq!(slot.hudless_commands, first);
    }

    #[test]
    fn address_reuse_discards_only_abandoned_recordings() {
        let mut slots: [Slot; SLOTS] = unsafe { std::mem::zeroed() };
        let mut plan: SubmitPlan = unsafe { std::mem::zeroed() };
        let command = ptr::without_provenance_mut(1);
        slots[0].recording_sequence = 1;
        slots[0].guides = 1;
        slots[0].recorded_prepare = 1;
        slots[0].prepare_commands = command;
        slots[0].recorded_hudless = 1;
        slots[0].hudless_commands = command;
        slots[1].recording_sequence = 2;
        slots[1].recorded_prepare = 1;
        slots[1].prepare_commands = command;
        let states: [*mut Slot; SLOTS] = std::array::from_fn(|i| &raw mut slots[i]);
        unsafe { rsf_core_fgvk_submitted(states.as_ptr(), &command, 1, &mut plan) };
        assert_eq!(plan.fences, 1 << 1);
        assert_eq!(plan.discarded_prepare, 1);
        assert_eq!(plan.discarded_hudless, 1);
        assert_eq!(slots[0].guides, 0);
        assert_eq!(slots[1].submitted_prepare, 1);
        unsafe { rsf_core_fgvk_submitted(states.as_ptr(), &command, 1, &mut plan) };
        assert_eq!(
            (plan.fences, plan.discarded_prepare, plan.discarded_hudless),
            (0, 0, 0)
        );
    }
}
