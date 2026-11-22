// SPDX-License-Identifier: MIT
//! The imagespace, Effects and TAA seams of an ordinary World source for Fallout 4 AE 1.11.240.
//!
//! Rust owns the verified caller and argument selection of the three imagespace calls and of the
//! Effects range inside imagespace, the World's imagespace entry, the before and after Effects
//! sequence around the SR seam (publication.rs), the imagespace exit order of recovery
//! retirement, auxiliary drain and output resolution with its receipts and refusals, and the TAA
//! exclusion predicate with its bounded receipt. render_hooks.c keeps each typed detour, the
//! original calls, the return addresses, the LastError saves and restores, the typed native
//! manager read, the guarded auxiliary and resolution calls, the shared diagnostic budget, the
//! resolution receipt, the World fields and the scope guard that runs the imagespace exit when
//! the original unwinds (NativeImagespaceLeaves in render_hooks_platform.h). Refusal, source and
//! raster checks use the recovery leaves. See docs/research/rust-fallout4-render-hooks.md.

use super::World;
use super::publication::{self, Leaves as PublicationLeaves, SrOutcome};
use super::recovery::{self, Leaves as RecoveryLeaves};
use crate::resolution::owns_full_output;
use crate::sys::NativeEffectsCall as EffectsCall;
use crate::sys::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::sys::{
    RSF_FALLOUT4_STAGE_EFFECTS_RETURNED as EFFECTS_RETURNED, RSF_FALLOUT4_STAGE_PRE_SR as PRE_SR,
    RSF_FALLOUT4_STAGE_UI_RESTORED as UI_RESTORED, RSF_FALLOUT4_STAGE_UI_RETURN as UI_RETURN,
};
use crate::{RSF_ERROR_NOT_READY, RSF_OK, image, message};
use rsf_game_sys::{rsf_game_render_pass, rsf_result};
use std::ffi::{CStr, c_char, c_void};
use std::mem::offset_of;
use std::sync::atomic::{AtomicU32, Ordering};

// Returns of the three selected imagespace calls (WORLD_CALLS in render_hooks.rs).
const IMAGESPACE_RETURNS: [usize; 3] = [
    super::return_rva(super::IMAGESPACE_CALLS[0]),
    super::return_rva(super::IMAGESPACE_CALLS[1]),
    super::return_rva(super::IMAGESPACE_CALLS[2]),
];
// Return of the Effects call inside imagespace (call 0x21fa283) and its exact range arguments.
const EFFECTS_RETURN: usize = super::return_rva(super::EFFECTS_CALL);
const EFFECTS_FIRST: u32 = 0;
const EFFECTS_LAST: u32 = 13;
const EFFECTS_MODE: u32 = 1;
const EFFECTS_FLAGS: u32 = 1;

const _: () = {
    assert!(size_of::<EffectsCall>() == 32);
    assert!(offset_of!(EffectsCall, first) == 16);
    assert!(offset_of!(EffectsCall, flags) == 28);
};

/// NativeImagespaceLeaves, constant for the process. Every leaf acts on the calling thread's
/// World.
#[repr(C)]
pub(super) struct Leaves {
    world: unsafe extern "C" fn(&mut World),
    // Mark the World inside the selected imagespace.
    enter: unsafe extern "C" fn(),
    // Clear the World's imagespace, temporal publication and full-output flags.
    leave: unsafe extern "C" fn(),
    // The typed read of the native imagespace manager cell; false when unreadable.
    manager: unsafe extern "C" fn(&mut usize) -> bool,
    // The guarded auxiliary restore; true when none is open or it drained.
    drain_auxiliaries: unsafe extern "C" fn() -> bool,
    // end_output_resolution.
    end_output: unsafe extern "C" fn() -> bool,
    // The resolution receipt and the render hooks' shared diagnostic budget, as the publication
    // and scope leaves bind them.
    report_state:
        unsafe extern "C" fn(u32, *const rsf_game_render_pass, *mut c_void, bool, rsf_result),
    diagnostic: unsafe extern "C" fn(*const c_char),
}

const _: () = assert!(size_of::<Leaves>() == usize::BITS as usize);

impl Leaves {
    fn world(&self) -> World {
        World::read(self.world)
    }
    fn manager(&self) -> Option<usize> {
        let mut manager = 0;
        (unsafe { (self.manager)(&mut manager) } && manager != 0).then_some(manager)
    }
    /// The resolution receipt on the World's state pass.
    fn report_state(&self, stage: u32, result: rsf_result) {
        let world = self.world();
        unsafe { (self.report_state)(stage, world.state(), world.context, world.published, result) }
    }
    fn diagnostic(&self, text: &CStr) {
        unsafe { (self.diagnostic)(text.as_ptr()) }
    }
}

/// The caller is one of the three verified imagespace calls, the World has not entered
/// imagespace, FRAME is open, and the callbacks are live on the current source or a recovery
/// token is owned. A selected call enters imagespace before the original runs.
fn enter_imagespace(leaves: &Leaves, recovery: &RecoveryLeaves, caller: usize) -> bool {
    let world = leaves.world();
    let base = image();
    let selected = !world.in_imagespace
        && world.frame_open
        && ((super::live() && recovery.same_source()) || recovery::token() != 0)
        && IMAGESPACE_RETURNS.iter().any(|&rva| caller == base + rva);
    if selected {
        unsafe { (leaves.enter)() };
    }
    selected
}

/// After a selected imagespace returns or unwinds: RETIRE recovery, drain auxiliaries, end the
/// full-output scope between its two receipts, refuse while output state remains owned, then
/// leave imagespace.
fn leave_imagespace(leaves: &Leaves, recovery: &RecoveryLeaves) {
    if !recovery::retire(recovery) {
        leaves.diagnostic(
            c"Fallout4 reduced recovery remains owned after imagespace; next reduction blocked.",
        );
    }
    if !unsafe { (leaves.drain_auxiliaries)() } {
        leaves.diagnostic(
            c"Fallout4 auxiliary retirement remains pending after native imagespace; owner retained.",
        );
    }
    leaves.report_state(UI_RETURN, RSF_OK);
    let result = if unsafe { (leaves.end_output)() } {
        RSF_OK
    } else {
        recovery.refuse();
        RSF_ERROR_NOT_READY
    };
    leaves.report_state(UI_RESTORED, result);
    if owns_full_output() {
        recovery.refuse();
        leaves.diagnostic(
            c"Fallout4 native output-state cleanup remains owned; subsequent world admission blocked.",
        );
    }
    unsafe { (leaves.leave)() };
}

/// The verified Effects call inside imagespace with its exact range arguments.
fn imagespace_effects(call: &EffectsCall) -> bool {
    call.caller == image() + EFFECTS_RETURN
        && call.first == EFFECTS_FIRST
        && call.last == EFFECTS_LAST
        && call.mode == EFFECTS_MODE
        && call.flags == EFFECTS_FLAGS
}

/// Before the original Effects range: on the live or recovery-owned imagespace call whose manager
/// is the native imagespace manager, the pre-SR receipt, then the SR seam.
fn before_effects(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    call: &EffectsCall,
    emit_sr: impl FnOnce() -> SrOutcome,
) -> Option<CpuIdentity> {
    if !imagespace_effects(call) {
        return None;
    }
    let world = leaves.world();
    if !world.in_imagespace
        || !world.frame_open
        || world.cpu.source == 0
        || leaves.manager() != Some(call.manager)
    {
        return None;
    }
    // An already-owned transaction still needs the Effects cleanup boundary after quiesce
    // or source invalidation. The return must still name this same World.
    if !(recovery::token() != 0
        || world.auxiliary_open
        || (super::live() && recovery.same_source()))
    {
        return None;
    }
    unsafe {
        (leaves.report_state)(
            PRE_SR,
            world.frame_pass,
            world.context,
            world.published,
            RSF_ERROR_NOT_READY,
        )
    };
    if emit_sr() == SrOutcome::Failed {
        leaves.diagnostic(c"Fallout4 SR callback failed; native imagespace retained.");
    }
    Some(world.cpu)
}

/// After the original Effects range of the imagespace call: the Effects receipt, RETIRE an owned
/// recovery token (its VATS original), then drain open auxiliaries.
fn after_effects(leaves: &Leaves, recovery: &RecoveryLeaves, receipt: &CpuIdentity) {
    let world = leaves.world();
    if !world.in_imagespace || !world.frame_open || world.cpu != *receipt {
        return;
    }
    leaves.report_state(EFFECTS_RETURNED, RSF_OK);
    if recovery::token() != 0 && !recovery::retire(recovery) {
        leaves.diagnostic(
            c"Fallout4 recovery VATS retirement refused at original Effects return; ownership retained.",
        );
    }
    if leaves.world().auxiliary_open && !unsafe { (leaves.drain_auxiliaries)() } {
        leaves.diagnostic(
            c"Fallout4 VATS/auxiliary restoration refused at native Effects return; owner retained.",
        );
    }
}

// One bit per observed TAA gate combination.
static TAA_RECEIPTS: AtomicU32 = AtomicU32::new(0);

/// The native TAA answer, excluded while the World's imagespace owns resolved full output on the
/// current raster snapshot (or, without one, the current source). The first sighting of each
/// gate combination is reported.
fn gate_taa(leaves: &Leaves, recovery: &RecoveryLeaves, original: bool) -> bool {
    let world = leaves.world();
    let owner_current = world.in_imagespace
        && world.full_output_resolved
        && if world.raster_valid {
            unsafe { (recovery.raster_current)() }
        } else {
            recovery.same_source()
        };
    let excluded = owner_current;
    let active = original && !excluded;
    let key = u32::from(original)
        | (u32::from(world.in_imagespace) << 1)
        | (u32::from(world.full_output_resolved) << 2)
        | (u32::from(world.published) << 3)
        | (u32::from(excluded) << 4);
    let bit = 1u32 << key;
    if TAA_RECEIPTS.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        // SAFETY: the World names its FRAME pass.
        let native = unsafe { world.frame_pass.as_ref() }.map_or(0, |frame| frame.native_frame);
        let text = message(format!(
            "Fallout4 TAA gate source={} native={} original={} imagespace={} full_output={} temporal={} owner_current={} native_taa_runs={} recovery={}",
            world.cpu.source,
            native,
            u32::from(original),
            u32::from(world.in_imagespace),
            u32::from(world.full_output_resolved),
            u32::from(world.published),
            u32::from(owner_current),
            u32::from(active),
            recovery::token(),
        ));
        recovery.report(&text);
    }
    active
}

/// Selects the imagespace call and enters imagespace before the original.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_enter_imagespace(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    caller: usize,
) -> bool {
    enter_imagespace(leaves, recovery, caller)
}

/// The exit of a selected imagespace, from the C++ scope guard after the original.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_leave_imagespace(leaves: &Leaves, recovery: &RecoveryLeaves) {
    leave_imagespace(leaves, recovery)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_effects(
    leaves: &Leaves,
    publication: &PublicationLeaves,
    recovery: &RecoveryLeaves,
    call: &EffectsCall,
    receipt: &mut CpuIdentity,
) -> bool {
    let Some(admitted) = before_effects(leaves, recovery, call, || {
        publication::emit_sr(publication, recovery)
    }) else {
        return false;
    };
    *receipt = admitted;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_effects(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    receipt: &CpuIdentity,
) {
    after_effects(leaves, recovery, receipt)
}

/// The TAA active answer after the original query.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_gate_taa(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    original: bool,
) -> bool {
    gate_taa(leaves, recovery, original)
}

#[cfg(test)]
mod tests;
