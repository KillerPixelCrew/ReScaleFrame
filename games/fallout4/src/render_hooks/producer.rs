// SPDX-License-Identifier: MIT
//! The viewport selector, dynamic-resolution writer and depth prepass seams of an ordinary World
//! source for Fallout 4 AE 1.11.240.
//!
//! Rust owns the choices and their order: the owned full-output selector write after the native
//! viewport intent on the native targets and its refusal, the HUDless receipt and seam after the
//! live HUDless selection inside imagespace, the depth invalidation before the DR writer, the
//! overlapping DR refusal and ownership transfer, the DR configuration admission on the current
//! source, screen and leased owner with its refusal, the nested prepass selection, the snapshot
//! invalidation and interruption before the original prepass, and the same-source raster and
//! depth capture after it with its refusal. render_hooks.c keeps each typed detour, the
//! original calls, the return addresses, the LastError saves and restores, the nested prepass
//! counter and its guard, the typed screen, owner, resolution and depth reads, the World fields
//! and snapshots (NativeProducerLeaves in render_hooks_platform.h). Refusal and source checks use
//! the recovery leaves. See docs/research/rust-fallout4-render-hooks.md.

use super::World;
use super::publication::{self, Leaves as PublicationLeaves};
use super::recovery::{self, Leaves as RecoveryLeaves};
use super::scope::Owner;
use crate::cpu_events::rsf_fallout4_cpu_render_source_is_current;
use crate::depth_domains::{EMPTY, Snapshot};
use crate::resolution::HUDLESS;
use crate::resolution::{self, TARGETS};
use crate::screen::WORLD_LEASE_INVALID;
use crate::sys::NativePrepassCall as PrepassCall;
use crate::sys::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::{RSF_OK, image};
use rsf_game_sys::{rsf_game_render_pass, rsf_result};
use std::ffi::{CStr, c_char, c_void};
use std::mem::offset_of;

// Return of the HUDless viewport selection inside imagespace (call 0x21fa2c5, WORLD_CALLS).
const HUDLESS_RETURN: usize = super::return_rva(super::HUDLESS_CALL);
// Returns of the selected DR writer (call 0x21f9f2f) and prepass (call 0x21f9f73) calls
// (PRODUCER_CALLS).
const DYNAMIC_RESOLUTION_RETURN: usize = super::return_rva(super::DR_CALL);
const PREPASS_RETURN: usize = super::return_rva(super::PREPASS_CALL);

const _: () = {
    assert!(size_of::<PrepassCall>() == 48 && offset_of!(PrepassCall, selected) == 40);
};

/// NativeProducerLeaves, constant for the process. Every leaf acts on the calling thread's World.
#[repr(C)]
struct Leaves {
    world: unsafe extern "C" fn(&mut World),
    // Clear the depth snapshot, and the raster snapshot when raster is set.
    invalidate: unsafe extern "C" fn(bool),
    // Set the World's depth interruption.
    interrupt_depth: unsafe extern "C" fn(bool),
    // Refuse the World and its raster.
    refuse_raster: unsafe extern "C" fn(),
    // native_resolution_write after the complete original DR writer.
    native_dr: unsafe extern "C" fn() -> bool,
    // The native screen refusals of the World source on an empty pass in the ordinary World.
    screen_refusals: unsafe extern "C" fn() -> u32,
    // The typed native owner read the scope leaves bind.
    read_owner: unsafe extern "C" fn(&mut Owner) -> bool,
    // configure_native_resolution on the FRAME pass and World Context with the recovery reserve.
    configure: unsafe extern "C" fn() -> bool,
    // The raster capture into the World raster snapshot, and the depth capture into a caller
    // snapshot, both for the given source.
    capture_raster: unsafe extern "C" fn(&CpuIdentity) -> bool,
    capture_depth: unsafe extern "C" fn(&CpuIdentity, &mut Snapshot) -> bool,
    // Copy a snapshot into the World depth snapshot.
    adopt_depth: unsafe extern "C" fn(&Snapshot),
    // The resolution receipt and the render hooks' shared diagnostic budget, as the publication
    // leaves bind them.
    report_state:
        unsafe extern "C" fn(u32, *const rsf_game_render_pass, *mut c_void, bool, rsf_result),
    diagnostic: unsafe extern "C" fn(*const c_char),
}

const _: () = assert!(size_of::<Leaves>() == 13 * size_of::<usize>());

impl Leaves {
    fn world(&self) -> World {
        World::read(self.world)
    }
    fn read_owner(&self) -> Option<Owner> {
        let mut owner = Owner::default();
        unsafe { (self.read_owner)(&mut owner) }.then_some(owner)
    }
    fn refuse_raster(&self) {
        unsafe { (self.refuse_raster)() }
    }
    fn diagnostic(&self, text: &CStr) {
        unsafe { (self.diagnostic)(text.as_ptr()) }
    }
}

/// After the original viewport selector store: on the native targets the owned full-output
/// selector follows the native intent, refusing when it cannot; on the live HUDless selection
/// inside imagespace the `hudless` receipt, then the HUDless seam.
fn after_viewport(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    caller: usize,
    manager: usize,
    enabled: bool,
    viewport_write: impl FnOnce(bool) -> bool,
    emit_hudless: impl FnOnce(),
) {
    let base = image();
    let targets = manager == base + TARGETS;
    if targets && !viewport_write(enabled) {
        recovery.refuse();
        leaves.diagnostic(
            c"Fallout4 owned full-output selector write failed after native viewport intent; scope retained.",
        );
    }
    if super::live() && targets && caller == base + HUDLESS_RETURN {
        let world = leaves.world();
        let state = world.state();
        unsafe { (leaves.report_state)(HUDLESS, state, world.context, world.published, RSF_OK) };
        emit_hudless();
    }
}

/// Before the original DR writer: an open FRAME drops its depth snapshot.
fn before_dynamic_resolution(leaves: &Leaves) {
    if leaves.world().frame_open {
        unsafe { (leaves.invalidate)(false) };
    }
}

/// The native owner still names the FRAME's frame, view and window keys and the World Context
/// and chain.
fn leased(world: &World, owner: &Owner) -> bool {
    // SAFETY: the World names its FRAME pass.
    let frame = unsafe { &*world.frame_pass };
    u64::from(owner.frame) == frame.native_frame
        && owner.view as u64 == frame.view_key
        && owner.window as u64 == frame.window_key
        && owner.context == world.context as usize
        && owner.chain == world.chain as usize
}

/// After the original DR writer. A call on the native targets while resolution ownership is
/// pending refuses the World raster and transfers the native write. The live selected call on
/// an open FRAME and SUBMISSION before inputs, on the current source, whose only screen refusal
/// is the World lease and whose native owner is the leased one, configures native resolution and
/// refuses the raster when that fails with ownership pending or a refused change. Any other call
/// on an open FRAME with pending ownership refuses the raster.
fn after_dynamic_resolution(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    caller: usize,
    manager: usize,
    busy: impl Fn() -> bool,
    refused: impl FnOnce() -> bool,
) {
    let base = image();
    let targets = manager == base + TARGETS;
    let overlapping = targets && busy();
    if overlapping {
        leaves.refuse_raster();
        if !unsafe { (leaves.native_dr)() } {
            leaves.diagnostic(
                c"Fallout4 native DR ownership transfer refused; pending scope retained.",
            );
        }
    }
    let world = leaves.world();
    if super::live()
        && caller == base + DYNAMIC_RESOLUTION_RETURN
        && targets
        && !overlapping
        && world.frame_open
        && world.submission_open
        && !world.inputs
        && recovery.same_source()
    {
        if unsafe { (leaves.screen_refusals)() } == WORLD_LEASE_INVALID
            && leaves
                .read_owner()
                .is_some_and(|owner| leased(&world, &owner))
            && !unsafe { (leaves.configure)() }
            && (busy() || refused())
        {
            leaves.refuse_raster();
        }
    } else if world.frame_open && busy() {
        leaves.refuse_raster();
    }
}

/// Before the original prepass, with the detour's nested depth: records the World source and
/// selects the live or recovery-owned outermost call from the verified site on an open FRAME and
/// SUBMISSION before inputs. Every call on an open FRAME, including foreign views, invalidates
/// both snapshots, a nested one interrupts depth and one after inputs refuses the World; the
/// selected call clears the interruption.
fn before_prepass(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    caller: usize,
    depth: u32,
) -> PrepassCall {
    let world = leaves.world();
    // FRAME admission records the ordinary parent beyond WorldBegin's return.
    let selected = (super::live() || recovery::token() != 0)
        && depth == 1
        && caller == image() + PREPASS_RETURN
        && world.frame_open
        && world.submission_open
        && !world.inputs;
    if world.frame_open {
        unsafe { (leaves.invalidate)(true) };
        if depth != 1 {
            unsafe { (leaves.interrupt_depth)(true) };
        }
        if world.inputs {
            recovery.refuse();
        }
    }
    if selected {
        unsafe { (leaves.interrupt_depth)(false) };
    }
    PrepassCall {
        identity: world.cpu,
        selected,
    }
}

/// After the original selected prepass, still live or recovery-owned on an open FRAME and
/// SUBMISSION before inputs and not interrupted: the raster capture, then a depth capture adopted
/// only when the recorded source is still the current render source and the World's source;
/// otherwise the World is refused.
fn after_prepass(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    call: &PrepassCall,
    source_current: impl FnOnce(&CpuIdentity) -> bool,
) {
    if !call.selected || !(super::live() || recovery::token() != 0) {
        return;
    }
    let world = leaves.world();
    if !world.frame_open || !world.submission_open || world.inputs || world.depth_interrupted {
        return;
    }
    let _ = unsafe { (leaves.capture_raster)(&call.identity) };
    // The stack-owned, value-initialized snapshot.
    let mut snapshot = EMPTY;
    if unsafe { (leaves.capture_depth)(&call.identity, &mut snapshot) }
        && source_current(&call.identity)
        && recovery.same_source()
    {
        unsafe { (leaves.adopt_depth)(&snapshot) };
    } else {
        recovery.refuse();
        leaves.diagnostic(
            c"Fallout4 native prepass returned without a valid same-source depth-domain snapshot.",
        );
    }
}

/// After the original viewport selector store.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_viewport(
    leaves: &Leaves,
    publication: &PublicationLeaves,
    recovery: &RecoveryLeaves,
    caller: usize,
    manager: usize,
    enabled: bool,
) {
    after_viewport(
        leaves,
        recovery,
        caller,
        manager,
        enabled,
        resolution::native_viewport,
        || publication::emit_hudless(publication, recovery),
    )
}

/// Before the original DR writer.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_dynamic_resolution(leaves: &Leaves) {
    before_dynamic_resolution(leaves)
}

/// After the original DR writer.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_dynamic_resolution(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    caller: usize,
    manager: usize,
) {
    after_dynamic_resolution(
        leaves,
        recovery,
        caller,
        manager,
        resolution::is_busy,
        resolution::refused,
    )
}

/// Before the original prepass; `call` receives the recorded source and the selection.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_prepass(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    caller: usize,
    depth: u32,
    call: &mut PrepassCall,
) {
    *call = before_prepass(leaves, recovery, caller, depth);
}

/// After the original prepass, with the call the before step recorded.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_prepass(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    call: &PrepassCall,
) {
    after_prepass(leaves, recovery, call, |identity| unsafe {
        rsf_fallout4_cpu_render_source_is_current(identity)
    })
}

#[cfg(test)]
mod tests;
