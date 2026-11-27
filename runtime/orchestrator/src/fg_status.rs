// SPDX-License-Identifier: MIT
use crate::sys::{
    RSF_FG_OFF, RSF_FG_STAT_ACTIVITY, RSF_FG_STAT_GENERATED_PRESENTED,
    RSF_FG_STAT_PRESENT_CALLBACKS, RSF_FG_STAT_TOTAL_PRESENTED, RSF_NATIVE_FG_REASON_NONE,
    rsf_fg_status as ProviderStatus, rsf_native_fg_status as NativeStatus,
};

const PRESENT_STATISTICS: u32 = RSF_FG_STAT_TOTAL_PRESENTED
    | RSF_FG_STAT_GENERATED_PRESENTED
    | RSF_FG_STAT_PRESENT_CALLBACKS
    | RSF_FG_STAT_ACTIVITY;
const EMPTY_PROVIDER: ProviderStatus = unsafe { std::mem::zeroed() };
const EMPTY_NATIVE: NativeStatus = unsafe { std::mem::zeroed() };

pub(crate) fn completed_activity(
    mut fresh: ProviderStatus,
    completed: Option<&ProviderStatus>,
    eligible: bool,
) -> ProviderStatus {
    let observed = completed.unwrap_or(&EMPTY_PROVIDER);
    fresh.valid_statistics = (fresh.valid_statistics & !PRESENT_STATISTICS)
        | (observed.valid_statistics & PRESENT_STATISTICS);
    fresh.total_presented = observed.total_presented;
    fresh.generated_presented = observed.generated_presented;
    fresh.present_callbacks = observed.present_callbacks;
    fresh.generated_callbacks = observed.generated_callbacks;
    if completed.is_some()
        && eligible
        && fresh.supported != 0
        && fresh.max_generated_frames != 0
        && fresh.configured_mode != RSF_FG_OFF
        && fresh.vendor_status == 0
    {
        fresh.active = observed.active;
        fresh.effective_mode = observed.effective_mode;
        fresh.effective_generated_frames = observed.effective_generated_frames;
    } else {
        deactivate(&mut fresh);
    }
    fresh
}

pub(crate) fn deactivate(status: &mut ProviderStatus) {
    status.active = 0;
    status.effective_mode = RSF_FG_OFF;
    status.effective_generated_frames = 0;
}

fn matches(completed_generation: u64, current_generation: u64, source: u64) -> bool {
    completed_generation != 0 && completed_generation == current_generation && source != 0
}

// The status pending at Prepare becomes the completed one only when its own Present succeeded under
// the same provider generation. Returns that generation (zero when refused) and the snapshot.
pub(crate) fn observe_completed(
    pending: &NativeStatus,
    pending_generation: u64,
    present_generation: u64,
    source_id: u64,
    success: bool,
    current_generation: u64,
) -> (u64, NativeStatus) {
    let accepted = success
        && source_id != 0
        && pending.source_frame_id == source_id
        && pending_generation == current_generation
        && present_generation == current_generation;
    if accepted {
        (current_generation, *pending)
    } else {
        (0, EMPTY_NATIVE)
    }
}

pub(crate) fn coherent(
    pending: &NativeStatus,
    pending_generation: u64,
    completed: &NativeStatus,
    completed_generation: u64,
    generation: u64,
    owner: bool,
    requested: bool,
) -> NativeStatus {
    let same = owner && matches(completed_generation, generation, completed.source_frame_id);
    let eligible = owner
        && requested
        && pending_generation == generation
        && pending.reason == RSF_NATIVE_FG_REASON_NONE
        && pending.last_result == 0;
    let mut value = if same && eligible {
        *completed
    } else {
        *pending
    };
    value.vendor = completed_activity(pending.vendor, same.then_some(&completed.vendor), eligible);
    if pending_generation != generation && !same {
        value.source_frame_id = 0;
        value.input_qpc = 0;
        value.source_qpc = 0;
        value.source_qpc_frequency = 0;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::RSF_FG_FIXED;
    use crate::sys::RSF_FG_STAT_LATENCY_AVAILABLE;
    use rsf_game_sys::backend_result::FEATURE_FAILED;

    #[test]
    fn completed_status_contract() {
        let mut pending = EMPTY_NATIVE;
        pending.struct_size = size_of::<NativeStatus>() as u32;
        pending.source_frame_id = 11;
        pending.vendor.struct_size = size_of::<ProviderStatus>() as u32;
        pending.vendor.supported = 1;
        pending.vendor.max_generated_frames = 4;
        pending.vendor.version_id = 99;
        pending.vendor.configured_mode = RSF_FG_FIXED;
        pending.vendor.active = 0;
        pending.vendor.total_presented = 100;
        pending.vendor.valid_statistics = RSF_FG_STAT_LATENCY_AVAILABLE;
        let mut completed = pending;
        let completed_generation = 7;
        completed.source_frame_id = 10;
        completed.source_qpc = 1000;
        completed.vendor.active = 1;
        completed.vendor.effective_mode = RSF_FG_FIXED;
        completed.vendor.effective_generated_frames = 1;
        completed.vendor.total_presented = 212;
        completed.vendor.generated_callbacks = 106;
        completed.vendor.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
        let view =
            |pending: &NativeStatus, completed: &NativeStatus, generation, owner, requested| {
                coherent(
                    pending,
                    7,
                    completed,
                    completed_generation,
                    generation,
                    owner,
                    requested,
                )
            };
        let status = view(&pending, &completed, 7, true, true);
        assert!(status.source_frame_id == 10 && status.source_qpc == 1000);
        assert!(status.vendor.active == 1 && status.vendor.total_presented == 212);
        assert_eq!(status.vendor.generated_callbacks, 106);
        assert!(status.vendor.max_generated_frames == 4 && status.vendor.version_id == 99);
        assert_ne!(
            status.vendor.valid_statistics & RSF_FG_STAT_LATENCY_AVAILABLE,
            0
        );
        // A pending provider query cannot replace completed activity, but capabilities stay live.
        let mut fresh = pending.vendor;
        fresh.max_generated_frames = 2;
        let merged = completed_activity(fresh, Some(&status.vendor), true);
        assert!(merged.active == 1 && merged.total_presented == 212);
        assert_eq!(merged.max_generated_frames, 2);
        // Reset, unsupported scene and no match.
        for reason in [5, 4, 1] {
            pending.reason = reason;
            let status = view(&pending, &completed, 7, true, true);
            assert!(status.vendor.active == 0 && status.reason == reason);
            assert_eq!(status.source_frame_id, 11);
        }
        pending.reason = 0;
        pending.last_result = FEATURE_FAILED;
        assert_eq!(view(&pending, &completed, 7, true, true).vendor.active, 0);
        pending.last_result = 0;
        assert_eq!(view(&pending, &completed, 7, true, false).vendor.active, 0);
        let status = view(&pending, &completed, 8, true, true);
        assert!(status.vendor.active == 0 && status.vendor.total_presented == 0);
        assert_eq!(status.source_frame_id, 0);
        assert_eq!(view(&pending, &completed, 7, false, true).vendor.active, 0);
        let receipt = |source, present_generation, success| {
            let (generation, status) =
                observe_completed(&completed, 7, present_generation, source, success, 7);
            (matches(generation, 7, status.source_frame_id), status)
        };
        // Completion source differs from the prepared source.
        assert!(!receipt(11, 7, true).0);
        assert!(!receipt(10, 8, true).0);
        assert!(!receipt(10, 7, false).0);
        let (matched, observed) = receipt(10, 7, true);
        assert!(matched && observed.vendor.total_presented == 212);
        deactivate(&mut completed.vendor);
        let status = view(&pending, &completed, 7, true, true);
        // Off/On needs a new completion.
        assert!(status.vendor.active == 0 && status.vendor.total_presented == 212);
        fresh.supported = 0;
        assert_eq!(completed_activity(fresh, Some(&merged), true).active, 0);
        fresh.supported = 1;
        fresh.configured_mode = RSF_FG_OFF;
        assert_eq!(completed_activity(fresh, Some(&merged), true).active, 0);
    }
}
