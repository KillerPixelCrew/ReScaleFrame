// SPDX-License-Identifier: MIT
//! The reduced-render recovery transaction for Fallout 4 AE 1.11.240.
//!
//! Rust owns the Runtime reduced-frame token through RESERVE, SEAL, FINISH, RETIRE and CANCEL,
//! the verdict taken from each host reply, the five-surface recovery plan, the output owner
//! identity recorded at reservation, the refusal and receipt budgets, and the retention counter
//! that keeps the render hooks busy while a token is owned. render_hooks.cpp keeps the typed
//! raster reads, COM identity, the held and candidate leases, the guarded Runtime call and the
//! World flags (NativeRecoveryLeaves in render_hooks_platform.h). The state is per thread, as the
//! C++ World is. See docs/research/rust-fallout4-render-hooks.md.

use crate::resolution::owns_full_output;
use crate::{RSF_ERROR_NOT_READY, RSF_OK, message};
use rsf_game_sys::{rsf_game_render_pass, rsf_result};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::mem::offset_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

// rsf_d3d11_reduced_frame (d3d11_host.h).
const ABI_VERSION: u32 = 1;
const RESERVE: u32 = 0;
const SEAL: u32 = 1;
const FINISH: u32 = 2;
const RETIRE: u32 = 3;
const CANCEL: u32 = 4;
const RESERVED: u32 = 1;
const FULL_OUTPUT_DEFINED: u32 = 0x1;
const TEMPORAL_OUTPUT: u32 = 0x2;
const NEXT_REDUCTION_ALLOWED: u32 = 0x8;
const TEMPORAL_ATTEMPTED: u32 = 0x40;

// rsf_d3d11_auxiliary_surface (d3d11_host.h).
const DEPTH_STENCIL: u32 = 0;
const COLOUR: u32 = 1;
const RESTORE_CONTENTS: u32 = 0x1;

// NativeRecoveryLeaseSlot.
const HELD: u32 = 0;
const CANDIDATE: u32 = 1;

/// RecoverySurfaceCount, in lease order: depth, motion, Pip-Boy, VATS, refraction normals.
const SURFACES: usize = 5;
const VATS: usize = 3;
const NAMES: [&CStr; SURFACES] = [
    c"native_depth_expanded",
    c"native_motion_expanded",
    c"native_pipboy_expanded",
    c"native_vats_expanded",
    c"native_refraction_expanded",
];

// Refusal receipts by stage, each reported once per process.
const REFUSALS: [&CStr; 8] = [
    c"Fallout4 reduction preflight refused: recovery export/current source unavailable or previous token remains owned.",
    c"Fallout4 reduction preflight refused: required native raster resources or full-output owner unavailable; native ratios retained.",
    c"Fallout4 reduction reservation refused or native owner/resources changed before ratio commitment.",
    c"Fallout4 committed recovery refused: same-frame native raster/resource producer unavailable; token retained.",
    c"Fallout4 committed recovery refused: resources/native output owner changed since reservation; token retained.",
    c"Fallout4 committed recovery SEAL refused; no temporal publication, token retained.",
    c"Fallout4 committed recovery FINISH withheld: native producer/output owner unverified; foreign resources untouched, token retained.",
    c"Fallout4 committed recovery full-output native scope refused; token retained.",
];
const FINISH_RECEIPTS: u32 = 32;

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // read by the Runtime
struct Surface {
    struct_size: u32,
    kind: u32,
    texture: *mut c_void,
    source_rect: [f32; 4],
    output_rect: [i32; 4],
    jitter_pixels: [f32; 2],
    name: *const c_char,
    source_name: *const c_char,
    flags: u32,
}

impl Surface {
    const EMPTY: Self = Self {
        struct_size: 0,
        kind: 0,
        texture: null_mut(),
        source_rect: [0.0; 4],
        output_rect: [0; 4],
        jitter_pixels: [0.0; 2],
        name: null(),
        source_name: null(),
        flags: 0,
    };
}

#[repr(C)]
#[allow(dead_code)] // read by the Runtime
pub(super) struct Request {
    struct_size: u32,
    abi_version: u32,
    operation: u32,
    disposition: u32,
    token: u64,
    owner: *const rsf_game_render_pass,
    surface_count: u32,
    reserved: u32,
    surfaces: *const Surface,
    temporal_result: i32,
    result_flags: u32,
}

impl Request {
    /// The C++ reduce leaf supplies the owner pass and Context of the held lease.
    fn new(operation: u32, token: u64, surfaces: &[Surface; SURFACES]) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version: ABI_VERSION,
            operation,
            disposition: 0,
            token,
            owner: null(),
            surface_count: SURFACES as u32,
            reserved: 0,
            surfaces: surfaces.as_ptr(),
            temporal_result: 0,
            result_flags: 0,
        }
    }
}

/// NativeOutputOwner (resolution.h): the native output scope a reservation was made against.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct OutputOwner {
    context: u64,
    manager: u64,
    triangles: [u64; 3],
}

/// NativeRecoveryFacts: the World values each step reads at the moment it runs.
#[repr(C)]
pub(super) struct Facts {
    context: *mut c_void,
    source: u64,
    exported: bool,
    frame_open: bool,
    full_output_resolved: bool,
}

/// NativeRecoveryLease: borrowed facts of a C++ lease, valid until that lease is replaced.
#[repr(C)]
pub(super) struct Lease {
    pass: *const rsf_game_render_pass,
    context: *mut c_void,
    textures: [*mut c_void; SURFACES],
}

const _: () = {
    assert!(size_of::<Surface>() == 80);
    assert!(offset_of!(Surface, source_rect) == 16);
    assert!(offset_of!(Surface, output_rect) == 32);
    assert!(offset_of!(Surface, name) == 56);
    assert!(offset_of!(Surface, flags) == 72);
    assert!(size_of::<Request>() == 56);
    assert!(offset_of!(Request, token) == 16);
    assert!(offset_of!(Request, surfaces) == 40);
    assert!(offset_of!(Request, result_flags) == 52);
    assert!(size_of::<OutputOwner>() == 40 && offset_of!(OutputOwner, triangles) == 16);
    assert!(size_of::<Facts>() == 24 && offset_of!(Facts, exported) == 16);
    assert!(size_of::<Lease>() == 7 * size_of::<usize>());
};

/// NativeRecoveryLeaves, constant for the process. Every leaf acts on the calling thread's World.
/// SR and HUDless publication (publication.rs) share the report, refusal, source and resolve
/// leaves.
#[repr(C)]
pub(super) struct Leaves {
    pub(super) report: unsafe extern "C" fn(*const c_char),
    pub(super) refuse: unsafe extern "C" fn(),
    pub(super) facts: unsafe extern "C" fn(&mut Facts),
    pub(super) same_source: unsafe extern "C" fn() -> bool,
    pub(super) resolve: unsafe extern "C" fn(bool, bool),
    pub(super) read_raster: unsafe extern "C" fn(bool) -> bool,
    pub(super) same_resources: unsafe extern "C" fn(bool) -> bool,
    pub(super) adopt: unsafe extern "C" fn(*const i32),
    pub(super) discard: unsafe extern "C" fn(),
    pub(super) release: unsafe extern "C" fn(),
    pub(super) lease: unsafe extern "C" fn(u32, &mut Lease),
    pub(super) preflight:
        unsafe extern "C" fn(*const rsf_game_render_pass, *mut c_void, &mut OutputOwner) -> bool,
    pub(super) raster_current: unsafe extern "C" fn() -> bool,
    pub(super) begin_output: unsafe extern "C" fn() -> bool,
    pub(super) reduce: unsafe extern "C" fn(&mut Request, &mut rsf_result) -> bool,
}

const _: () = assert!(size_of::<Leaves>() == 15 * size_of::<usize>());

impl Leaves {
    pub(super) fn report(&self, text: &CStr) {
        unsafe { (self.report)(text.as_ptr()) }
    }
    /// Reports a terminated message from `crate::message`.
    pub(super) fn report_text(&self, text: &CStr) {
        unsafe { (self.report)(text.as_ptr()) }
    }
    pub(super) fn refuse(&self) {
        unsafe { (self.refuse)() }
    }
    /// Sets the World's full-output and temporal publication flags.
    pub(super) fn resolve(&self, full_output: bool, published: bool) {
        unsafe { (self.resolve)(full_output, published) }
    }
    /// The World's current full-output flag.
    pub(super) fn full_output_resolved(&self) -> bool {
        self.facts().full_output_resolved
    }
    fn facts(&self) -> Facts {
        let mut facts = Facts {
            context: null_mut(),
            source: 0,
            exported: false,
            frame_open: false,
            full_output_resolved: false,
        };
        unsafe { (self.facts)(&mut facts) };
        facts
    }
    pub(super) fn same_source(&self) -> bool {
        unsafe { (self.same_source)() }
    }
    fn read_raster(&self, producer: bool) -> bool {
        unsafe { (self.read_raster)(producer) }
    }
    fn same_resources(&self, owners: bool) -> bool {
        unsafe { (self.same_resources)(owners) }
    }
    fn lease(&self, slot: u32) -> Lease {
        let mut lease = Lease {
            pass: null(),
            context: null_mut(),
            textures: [null_mut(); SURFACES],
        };
        unsafe { (self.lease)(slot, &mut lease) };
        lease
    }
    fn preflight(
        &self,
        pass: *const rsf_game_render_pass,
        context: *mut c_void,
        owner: &mut OutputOwner,
    ) -> bool {
        unsafe { (self.preflight)(pass, context, owner) }
    }
}

/// Discards the candidate lease when a step returns, as the C++ stack lease did.
struct Candidate<'a>(&'a Leaves);

impl Drop for Candidate<'_> {
    fn drop(&mut self) {
        unsafe { (self.0.discard)() }
    }
}

#[derive(Clone, Copy)]
struct Recovery {
    token: u64,
    committed: bool,
    sealed: bool,
    finished: bool,
    owner: OutputOwner,
    surfaces: [Surface; SURFACES],
}

impl Recovery {
    const EMPTY: Self = Self {
        token: 0,
        committed: false,
        sealed: false,
        finished: false,
        owner: OutputOwner {
            context: 0,
            manager: 0,
            triangles: [0; 3],
        },
        surfaces: [Surface::EMPTY; SURFACES],
    };
}

// The Runtime call and the native reads can reenter the hooks, so no borrow of this state is
// held across a leaf: each step reads a copy and stores its change after the call.
thread_local! {
    static RECOVERY: Cell<Recovery> = const { Cell::new(Recovery::EMPTY) };
}

// Owned tokens across threads; part of the render hooks' busy answer.
static OWNED: AtomicU32 = AtomicU32::new(0);
static REFUSED_STAGES: AtomicU32 = AtomicU32::new(0);
static FINISH_ROWS: AtomicU32 = AtomicU32::new(0);

fn state() -> Recovery {
    RECOVERY.get()
}

fn update(change: impl FnOnce(&mut Recovery)) {
    let mut recovery = RECOVERY.get();
    change(&mut recovery);
    RECOVERY.set(recovery);
}

/// A token is owned on some thread.
pub(super) fn owned() -> bool {
    OWNED.load(Ordering::Acquire) != 0
}

/// The calling thread's token, zero when none is owned.
pub(super) fn token() -> u64 {
    state().token
}

/// Reports the stage once, refuses the World's temporal output while a reservation or its
/// commitment exists, and answers false.
fn refusal(leaves: &Leaves, stage: usize) -> bool {
    let bit = 1u32 << stage;
    if REFUSED_STAGES.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        leaves.report(REFUSALS[stage]);
    }
    let recovery = state();
    if recovery.committed || recovery.token != 0 {
        leaves.refuse();
    }
    false
}

/// The recovery surfaces of the held lease.
fn plan(leaves: &Leaves) -> [Surface; SURFACES] {
    let lease = leaves.lease(HELD);
    // SAFETY: the held slot always names its World pass.
    let pass = unsafe { &*lease.pass };
    let mut surfaces = [Surface::EMPTY; SURFACES];
    for (index, surface) in surfaces.iter_mut().enumerate() {
        // Numeric nearest recovery keeps the native jittered integer lattice. These zero
        // operator offsets do not report zero native camera jitter.
        *surface = Surface {
            struct_size: size_of::<Surface>() as u32,
            kind: if index == 0 { DEPTH_STENCIL } else { COLOUR },
            texture: lease.textures[index],
            source_rect: pass.render_rect.map(|edge| edge as f32),
            output_rect: pass.output_rect,
            jitter_pixels: [0.0; 2],
            name: NAMES[index].as_ptr(),
            source_name: NAMES[index].as_ptr(),
            flags: if index == VATS { RESTORE_CONTENTS } else { 0 },
        };
    }
    surfaces
}

/// One operation on the owned token. FINISH sets the World's full-output and temporal verdict
/// from the host flags. A successful CANCEL or RETIRE that allows the next reduction drains the
/// token and releases the held lease; any other answer retains both.
fn call(leaves: &Leaves, operation: u32) -> bool {
    let recovery = state();
    if recovery.token == 0 {
        return false;
    }
    let mut request = Request::new(operation, recovery.token, &recovery.surfaces);
    let mut result = RSF_ERROR_NOT_READY;
    if !unsafe { (leaves.reduce)(&mut request, &mut result) } {
        return false;
    }
    if operation == FINISH {
        let full_output = request.result_flags & FULL_OUTPUT_DEFINED != 0;
        let published = full_output && request.result_flags & TEMPORAL_OUTPUT != 0;
        unsafe { (leaves.resolve)(full_output, published) };
        if !published {
            leaves.refuse();
        }
        if FINISH_ROWS.fetch_add(1, Ordering::Relaxed) < FINISH_RECEIPTS {
            // SAFETY: the reduce leaf names the held World pass.
            let native = unsafe { request.owner.as_ref() }.map_or(0, |owner| owner.native_frame);
            let text = message(format!(
                "Fallout4 reduced recovery source={} native={} result={} disposition={} flags=0x{:x} temporal_result={} temporal_attempted={}",
                leaves.facts().source,
                native,
                result,
                request.disposition,
                request.result_flags,
                request.temporal_result,
                u32::from(request.result_flags & TEMPORAL_ATTEMPTED != 0),
            ));
            unsafe { (leaves.report)(text.as_ptr()) };
        }
    }
    if result == RSF_OK
        && (operation == CANCEL || operation == RETIRE)
        && request.result_flags & NEXT_REDUCTION_ALLOWED != 0
    {
        update(|recovery| {
            recovery.token = 0;
            recovery.surfaces = [Surface::EMPTY; SURFACES];
        });
        unsafe { (leaves.release)() };
        OWNED.fetch_sub(1, Ordering::AcqRel);
    }
    result == RSF_OK
}

/// Preflight, RESERVE, then confirm the same source, output owner and resources before the
/// caller's first ratio write. A failed confirmation cancels the unused reservation.
fn reserve(leaves: &Leaves, planned: &rsf_game_render_pass) -> bool {
    let facts = leaves.facts();
    if !super::live()
        || !facts.exported
        || state().token != 0
        || !facts.frame_open
        || !leaves.same_source()
    {
        return refusal(leaves, 0);
    }
    let _candidate = Candidate(leaves);
    // The preflight records the output owner directly, as the C++ owner field did.
    let mut owner = state().owner;
    let preflighted =
        leaves.read_raster(false) && leaves.preflight(planned, facts.context, &mut owner);
    update(|recovery| recovery.owner = owner);
    if !preflighted {
        return refusal(leaves, 1);
    }
    unsafe { (leaves.adopt)(planned.render_rect.as_ptr()) };
    let surfaces = plan(leaves);
    update(|recovery| recovery.surfaces = surfaces);
    let mut request = Request::new(RESERVE, 0, &surfaces);
    let mut result = RSF_ERROR_NOT_READY;
    let _ = unsafe { (leaves.reduce)(&mut request, &mut result) };
    if request.token != 0 {
        update(|recovery| recovery.token = request.token);
        OWNED.fetch_add(1, Ordering::AcqRel);
    }
    let unchanged = result == RSF_OK
        && request.token != 0
        && request.disposition == RESERVED
        && super::live()
        && leaves.same_source()
        && {
            let mut owner = OutputOwner::default();
            leaves.preflight(planned, leaves.facts().context, &mut owner) && owner == state().owner
        }
        && leaves.read_raster(false)
        && leaves.same_resources(false);
    if !unchanged {
        if state().token != 0 {
            let _ = call(leaves, CANCEL);
        }
        return refusal(leaves, 2);
    }
    // Mark before the caller's first ratio write. Even a partial native-state write must retain
    // the reservation until explicit cancellation or retirement.
    update(|recovery| recovery.committed = true);
    true
}

/// Adopts the same-frame produced resources, which must match the reservation and its output
/// owner, then SEALs them before any temporal reader.
pub(super) fn seal(leaves: &Leaves) -> bool {
    let recovery = state();
    if recovery.token == 0 || recovery.sealed {
        return true;
    }
    let _candidate = Candidate(leaves);
    if !leaves.read_raster(true) {
        return refusal(leaves, 3);
    }
    let current = leaves.lease(CANDIDATE);
    let mut owner = OutputOwner::default();
    if !leaves.same_resources(true)
        || !leaves.preflight(current.pass, current.context, &mut owner)
        || owner != state().owner
    {
        return refusal(leaves, 4);
    }
    unsafe { (leaves.adopt)(null()) };
    let surfaces = plan(leaves);
    update(|recovery| recovery.surfaces = surfaces);
    if !call(leaves, SEAL) {
        return refusal(leaves, 5);
    }
    update(|recovery| recovery.sealed = true);
    true
}

/// FINISH a committed token once, inside the full-output scope. Answers whether full output was
/// resolved; a finished token answers the World's current full-output flag.
pub(super) fn finish(leaves: &Leaves) -> bool {
    let recovery = state();
    if recovery.token == 0 || !recovery.committed {
        return true;
    }
    if recovery.finished {
        return leaves.facts().full_output_resolved;
    }
    // Missing producer/owner proof is an unresolved committed frame, never an invitation to read
    // the new owner's resources or cancel the obligation.
    let held = leaves.lease(HELD);
    let mut owner = OutputOwner::default();
    if !recovery.sealed
        || !unsafe { (leaves.raster_current)() }
        || !leaves.preflight(held.pass, held.context, &mut owner)
        || owner != recovery.owner
    {
        return refusal(leaves, 6);
    }
    if !owns_full_output() && !unsafe { (leaves.begin_output)() } {
        return refusal(leaves, 7);
    }
    let okay = call(leaves, FINISH);
    let finished = okay && leaves.facts().full_output_resolved;
    update(|recovery| recovery.finished = finished);
    if !finished {
        leaves.refuse();
    }
    finished
}

/// CANCEL an uncommitted token; FINISH then RETIRE a committed one.
///
/// `finished` is this token's own proof that FINISH resolved full output. With it, RETIRE goes
/// straight to the host, so a retry after the imagespace exit cleared the World's
/// `full_output_resolved` still reaches it, and FINISH is never repeated. Without it, the guarded
/// FINISH admission runs first, as before.
pub(super) fn retire(leaves: &Leaves) -> bool {
    let recovery = state();
    if recovery.token == 0 {
        return true;
    }
    if !recovery.committed {
        return call(leaves, CANCEL);
    }
    if !recovery.finished && !finish(leaves) {
        return false;
    }
    call(leaves, RETIRE)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_recovery_reserve(
    leaves: &Leaves,
    planned: &rsf_game_render_pass,
) -> bool {
    reserve(leaves, planned)
}

/// The calling thread's token, zero when none is owned.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_recovery_token() -> u64 {
    state().token
}

/// The calling thread's reservation was committed before its first ratio write.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_recovery_committed() -> bool {
    state().committed
}

/// Clears the calling thread's state with its World, which resets only without a token.
pub(super) fn reset() {
    RECOVERY.set(Recovery::EMPTY);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_recovery_reset() {
    reset();
}

#[cfg(test)]
mod tests;
