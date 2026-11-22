// SPDX-License-Identifier: MIT
//! The FRAME, SUBMISSION and WINDOW scope of an ordinary World source for Fallout 4 AE 1.11.240.
//!
//! Rust owns the scope decisions and their order: the verified before/after native owner lease
//! at WorldBegin admission, the FRAME pass identity and keys, FRAME then SUBMISSION begin, the
//! balanced SUBMISSION close before the outer Present or after Swap, NO_FG on refusal, the
//! WINDOW then FRAME close order, the recovery, auxiliary and resolution continuation after Swap
//! and the conditional World reset. render_hooks.c keeps the typed native owner read, the COM
//! identity checks and the Context and chain leases, the scope counters, the guarded auxiliary
//! and resolution calls, the guarded pass callback, the shared diagnostic budget, the resolution
//! receipt and the World fields (NativeScopeLeaves in render_hooks_platform.h). Refusal, source
//! checks and recovery retirement use the recovery leaves. See
//! docs/research/rust-fallout4-render-hooks.md.

use super::World;
use super::publication;
use super::recovery::{self, Leaves as RecoveryLeaves};
use crate::resolution::is_busy as resolution_busy;
pub(super) use crate::sys::NativeRenderOwner as Owner;
use crate::sys::RSF_FALLOUT4_STAGE_SOURCE_RETIRED as SOURCE_RETIRED;
use crate::{RSF_ERROR_NOT_READY, RSF_OK, image};
use rsf_game_sys::{
    RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_SUBMISSION, rsf_game_render_pass,
    rsf_result,
};
use std::ffi::{CStr, c_char, c_void};
use std::mem::offset_of;
use std::sync::atomic::{AtomicBool, Ordering};

// WorldBegin, the FRAME and SUBMISSION pass key.
const FRAME_PASS: usize = 0x21f_0ac0;

const _: () = {
    assert!(size_of::<Owner>() == 64 && offset_of!(Owner, frame) == 56);
};

/// NativeScopeLeaves, constant for the process. Every leaf acts on the calling thread's World.
#[repr(C)]
pub(super) struct Leaves {
    pub(super) world: unsafe extern "C" fn(&mut World),
    // The typed native owner read, including the accepted window; false when any cell is
    // unreadable, null or not the recognized renderer.
    pub(super) read_owner: unsafe extern "C" fn(&mut Owner) -> bool,
    // COM-lease the owner's Context and chain as the candidate, then check an immediate Context
    // on the owner device, the chain on that device and the chain's output window.
    pub(super) lease_owner: unsafe extern "C" fn(&Owner) -> bool,
    // Move the candidate Context and chain into the World.
    pub(super) adopt_owner: unsafe extern "C" fn(),
    pub(super) discard_owner: unsafe extern "C" fn(),
    // Open FRAME and its counter; open SUBMISSION.
    pub(super) open_frame: unsafe extern "C" fn(),
    pub(super) open_submission: unsafe extern "C" fn(),
    // Clear the open flag, and the WINDOW or FRAME counter.
    pub(super) close_submission: unsafe extern "C" fn(),
    pub(super) close_window: unsafe extern "C" fn(),
    pub(super) close_frame: unsafe extern "C" fn(),
    // The guarded auxiliary restore; true when none is open or it drained.
    pub(super) drain_auxiliaries: unsafe extern "C" fn() -> bool,
    // finish_resolution_source.
    pub(super) finish_resolution: unsafe extern "C" fn() -> bool,
    // World = {}.
    pub(super) reset: unsafe extern "C" fn(),
    // The guarded host pass callback, the render hooks' shared diagnostic budget and the
    // resolution receipt, as the publication leaves bind them.
    pub(super) notify: unsafe extern "C" fn(*const rsf_game_render_pass, *mut c_void, u32),
    pub(super) diagnostic: unsafe extern "C" fn(*const c_char),
    pub(super) report_state:
        unsafe extern "C" fn(u32, *const rsf_game_render_pass, *mut c_void, bool, rsf_result),
}

const _: () = assert!(size_of::<Leaves>() == 16 * size_of::<usize>());

impl Leaves {
    fn world(&self) -> World {
        World::read(self.world)
    }
    fn read_owner(&self) -> Option<Owner> {
        let mut owner = Owner::default();
        unsafe { (self.read_owner)(&mut owner) }.then_some(owner)
    }
    /// # Safety
    /// `pass` is a World pass of the calling thread.
    unsafe fn notify(&self, pass: *const rsf_game_render_pass, context: *mut c_void, begin: u32) {
        unsafe { (self.notify)(pass, context, begin) }
    }
    fn diagnostic(&self, text: &CStr) {
        unsafe { (self.diagnostic)(text.as_ptr()) }
    }
}

/// Discards the candidate Context and chain when the begin step returns, as the C++ stack
/// leases ended.
struct Candidate<'a>(&'a Leaves);

impl Drop for Candidate<'_> {
    fn drop(&mut self) {
        unsafe { (self.0.discard_owner)() }
    }
}

static BEGIN_ANNOUNCED: AtomicBool = AtomicBool::new(false);

/// The owner read before and after the COM lease must match, and the source must be current,
/// before the Context and chain become the World's.
fn lease(scope: &Leaves, recovery: &RecoveryLeaves) -> Option<Owner> {
    let before = scope.read_owner()?;
    let _candidate = Candidate(scope);
    if !unsafe { (scope.lease_owner)(&before) }
        || scope.read_owner() != Some(before)
        || !recovery.same_source()
    {
        return None;
    }
    unsafe { (scope.adopt_owner)() };
    Some(before)
}

/// FRAME then SUBMISSION, opened before the original ordinary WorldBegin. Answers false without
/// opening either when the owner lease is refused.
fn begin(scope: &Leaves, recovery: &RecoveryLeaves) -> bool {
    let Some(owner) = lease(scope, recovery) else {
        return false;
    };
    let world = scope.world();
    // SAFETY: the World names its FRAME pass; the FRAME is built in place, as before.
    let mut frame = unsafe { *world.frame_pass };
    frame.struct_size = size_of::<rsf_game_render_pass>() as u32;
    publication::identify(&mut frame, &world.cpu);
    frame.role = RSF_GAME_RENDER_FRAME;
    frame.native_frame = u64::from(owner.frame);
    frame.family_key = owner.view as u64;
    frame.view_key = owner.view as u64;
    frame.history_key = owner.view as u64;
    frame.viewport_key = owner.window as u64;
    frame.window_key = owner.window as u64;
    frame.rhi_viewport_key = owner.window as u64;
    frame.pass_key = (image() + FRAME_PASS) as u64;
    frame.swapchain = owner.chain as *mut c_void;
    let mut submission = frame;
    submission.role = RSF_GAME_RENDER_SUBMISSION;
    // SAFETY: the World names its FRAME and SUBMISSION passes. The callbacks receive the World
    // fields themselves.
    unsafe {
        world.frame_pass.write(frame);
        world.submission_pass.write(submission);
        (scope.open_frame)();
        scope.notify(world.frame_pass, world.context, 1);
        (scope.open_submission)();
        scope.notify(world.submission_pass, world.context, 1);
    }
    if !BEGIN_ANNOUNCED.swap(true, Ordering::Relaxed) {
        recovery.report(
            c"Fallout4 native FRAME/SUBMISSION begin before ordinary WorldBegin; screen/input eligibility not inferred.",
        );
    }
    true
}

/// Ends an open SUBMISSION once, with NO_FG when the source is refused.
fn close_submission(scope: &Leaves) {
    let world = scope.world();
    if !world.submission_open {
        return;
    }
    // SAFETY: the World names its SUBMISSION pass.
    unsafe {
        if world.refused {
            (*world.submission_pass).flags |= RSF_GAME_RENDER_NO_FG;
        }
        scope.notify(world.submission_pass, world.context, 0);
        (scope.close_submission)();
    }
}

/// After the original Swap: balance a SUBMISSION the pre-Present guard withheld, end WINDOW then
/// FRAME, retire recovery, drain auxiliaries, finish the resolution source and reset the World
/// only when no obligation remains.
fn finish(scope: &Leaves, recovery: &RecoveryLeaves) {
    if scope.world().cpu.source == 0 {
        return;
    }
    if !recovery.same_source() {
        recovery.refuse();
    }
    // Refused or unpresented sources drain here; only the guarded pre-Present path can provide
    // completion in time for that source's generation prepare.
    if scope.world().submission_open {
        recovery.refuse();
        scope.diagnostic(
            c"Fallout4 refused/unpresented SUBMISSION balanced after Swap, before FRAME end.",
        );
        close_submission(scope);
    }
    let world = scope.world();
    // SAFETY: the World names its passes.
    unsafe {
        if world.refused {
            (*world.pass).flags |= RSF_GAME_RENDER_NO_FG;
        }
        if world.window_open {
            scope.notify(world.pass, world.context, 0);
            (scope.close_window)();
        }
    }
    let world = scope.world();
    if world.frame_open {
        // SAFETY: the World names its FRAME pass.
        unsafe {
            if world.refused {
                (*world.frame_pass).flags |= RSF_GAME_RENDER_NO_FG;
            }
            scope.notify(world.frame_pass, world.context, 0);
            (scope.close_frame)();
        }
    }
    let _ = recovery::retire(recovery);
    let _ = unsafe { (scope.drain_auxiliaries)() };
    let resolution_finished = unsafe { (scope.finish_resolution)() };
    if !resolution_finished {
        recovery.refuse();
    }
    let world = scope.world();
    let state = world.state();
    let result = if resolution_finished {
        RSF_OK
    } else {
        RSF_ERROR_NOT_READY
    };
    unsafe {
        (scope.report_state)(
            SOURCE_RETIRED,
            state,
            world.context,
            world.published,
            result,
        )
    };
    if !resolution_busy() && !scope.world().auxiliary_open && recovery::token() == 0 {
        unsafe { (scope.reset)() };
        recovery::reset();
    }
}

/// FRAME then SUBMISSION before the original ordinary WorldBegin, on a World the caller just
/// reset and bound to the admitted source. A refused owner lease refuses the World.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_begin_scope(scope: &Leaves, recovery: &RecoveryLeaves) {
    if !begin(scope, recovery) {
        recovery.refuse();
        scope.diagnostic(c"Fallout4 pre-world FRAME/SUBMISSION owner/view/frame lease refused.");
    }
}

/// The SUBMISSION end of an admitted complete outer Present.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_close_submission(scope: &Leaves) {
    close_submission(scope)
}

/// The CPU owner's post-Swap scope end.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_finish_scope(scope: &Leaves, recovery: &RecoveryLeaves) {
    finish(scope, recovery)
}

#[cfg(test)]
mod tests;
