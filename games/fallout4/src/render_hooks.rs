// SPDX-License-Identifier: MIT
//! Ordinary World and outer Present admission for Fallout 4 AE 1.11.240.

use crate::cpu_abi::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::cpu_events::{rsf_fallout4_cpu_begin_render, rsf_fallout4_cpu_render_source_is_current};
use crate::image;
use crate::resolution::rsf_fallout4_resolution_busy;
use std::mem::offset_of;

// Return addresses of the guarded MainDraw and Swap calls in verify_render_hooks.
const ORDINARY_WORLD_RETURN: usize = 0xc3_82a5;
const OUTER_PRESENT_RETURN: usize = 0xc3_35ec;
const PRESENT_RENDERER: usize = 0x3a2_5d00;

#[repr(C)]
struct WorldAdmission {
    caller: usize,
    session: u64,
    live: bool,
    ordinary_world: bool,
    frame_open: bool,
    auxiliary_open: bool,
    recovery_owned: bool,
}

#[repr(C)]
#[derive(Default, PartialEq, Eq)]
struct ScopeKey {
    frame: u64,
    view: u64,
    window: u64,
    context: usize,
    chain: usize,
}

#[repr(u32)]
#[derive(PartialEq, Eq)]
enum PresentMatch {
    Unrelated = 0,
    OwnerChanged = 1,
    Matched = 2,
}

#[repr(u32)]
enum PresentAdmission {
    SourceChanged = 0,
    Refused = 1,
    Complete = 2,
}

type ReadPresentOwner = unsafe extern "C" fn(&mut ScopeKey, &mut usize) -> bool;
type RevalidatePresentScreen = unsafe extern "C" fn() -> bool;

const _: () = {
    assert!(size_of::<WorldAdmission>() == 24);
    assert!(offset_of!(WorldAdmission, live) == 16);
    assert!(offset_of!(WorldAdmission, recovery_owned) == 20);
    assert!(size_of::<ScopeKey>() == 40);
    assert!(offset_of!(ScopeKey, context) == 24);
    assert!(offset_of!(ScopeKey, chain) == 32);
};

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_admit_world(
    candidate: &WorldAdmission,
    cpu: &CpuIdentity,
) -> bool {
    candidate.live
        && candidate.caller == image() + ORDINARY_WORLD_RETURN
        && cpu.session == candidate.session
        && cpu.source != 0
        && cpu.submission == 0
        && !candidate.ordinary_world
        && !candidate.frame_open
        && !candidate.auxiliary_open
        && !candidate.recovery_owned
        && !rsf_fallout4_resolution_busy()
        && rsf_fallout4_cpu_begin_render()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_match_present(
    caller: usize,
    renderer: usize,
    submission_open: bool,
    leased: &ScopeKey,
    read_owner: ReadPresentOwner,
) -> PresentMatch {
    let base = image();
    if caller != base + OUTER_PRESENT_RETURN
        || renderer != base + PRESENT_RENDERER
        || !submission_open
    {
        return PresentMatch::Unrelated;
    }
    let mut current = ScopeKey::default();
    let mut outer_chain = 0;
    if unsafe { read_owner(&mut current, &mut outer_chain) }
        && outer_chain == leased.chain
        && current == *leased
    {
        PresentMatch::Matched
    } else {
        PresentMatch::OwnerChanged
    }
}

// Called after the optional recovery capture, which only requires a matched native owner.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_admit_present(
    matched: PresentMatch,
    session: u64,
    cpu: &CpuIdentity,
    revalidate_screen: RevalidatePresentScreen,
) -> PresentAdmission {
    if matched != PresentMatch::Matched
        || cpu.session != session
        || !unsafe { rsf_fallout4_cpu_render_source_is_current(cpu) }
    {
        return PresentAdmission::SourceChanged;
    }
    // Revalidate any leased screen and keep earlier refusals sticky for the source.
    if unsafe { revalidate_screen() } {
        PresentAdmission::Refused
    } else {
        PresentAdmission::Complete
    }
}
