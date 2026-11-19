// SPDX-License-Identifier: MIT
use crate::render_abi::{
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

fn completed_activity(
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

fn deactivate(status: &mut ProviderStatus) {
    status.active = 0;
    status.effective_mode = RSF_FG_OFF;
    status.effective_generated_frames = 0;
}

fn matches(completed_generation: u64, current_generation: u64, source: u64) -> bool {
    completed_generation != 0 && completed_generation == current_generation && source != 0
}

// Internal bridges. The native caller owns the initialized snapshots and
// serializes status reads, Present observations and provider switches.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg_completed_activity(
    fresh: *const ProviderStatus,
    completed: *const ProviderStatus,
    same_provider: u32,
    eligible: u32,
    out: *mut ProviderStatus,
) {
    let fresh = unsafe { fresh.read() };
    let completed = (same_provider != 0).then(|| unsafe { &*completed });
    unsafe { out.write(completed_activity(fresh, completed, eligible != 0)) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg_observe_completed(
    pending: *const NativeStatus,
    pending_generation: u64,
    present_generation: u64,
    source_id: u64,
    success: u32,
    current_generation: u64,
    generation: *mut u64,
    status: *mut NativeStatus,
) {
    let accepted = success != 0
        && source_id != 0
        && unsafe { (*pending).source_frame_id } == source_id
        && pending_generation == current_generation
        && present_generation == current_generation;
    let snapshot = if accepted {
        unsafe { pending.read() }
    } else {
        EMPTY_NATIVE
    };
    unsafe {
        generation.write(if accepted { current_generation } else { 0 });
        status.write(snapshot);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg_deactivate_completed(status: *mut ProviderStatus) {
    deactivate(unsafe { &mut *status });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fg_completed_matches(completed: u64, current: u64, source: u64) -> u32 {
    u32::from(matches(completed, current, source))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg_coherent_status(
    pending: *const NativeStatus,
    pending_generation: u64,
    completed: *const NativeStatus,
    completed_generation: u64,
    generation: u64,
    owner: u32,
    requested: u32,
    out: *mut NativeStatus,
) {
    let pending = unsafe { &*pending };
    let completed = unsafe { &*completed };
    let same = owner != 0 && matches(completed_generation, generation, completed.source_frame_id);
    let eligible = owner != 0
        && requested != 0
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
    unsafe { out.write(value) };
}
