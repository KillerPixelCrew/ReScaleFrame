// SPDX-License-Identifier: MIT
//! SR and HUDless publication for Fallout 4 AE 1.11.240.
//!
//! Rust owns the admission and ordering of the SR seam inside the imagespace Effects call
//! (selected and sequenced by imagespace.rs) and of the HUDless viewport seam (selected by
//! producer.rs): the recovery SEAL before any temporal reader and its FINISH at the
//! end of the SR step, every refusal and its World refusal, the FRAME/SUBMISSION lease and
//! producer comparisons, the pass identity and role, the SR-off decision, the screen verdict, the
//! FG_INPUTS, FINAL_SCENE and WINDOW callback order, the scalar identity kept after FG input
//! publication, the scoped output resolution calls and their receipts. render_hooks.cpp keeps the
//! SR input references, engine depth and screen reads, Runtime exception guards and
//! the World fields (NativePublicationLeaves in
//! render_hooks_platform.h). See docs/research/rust-fallout4-render-hooks.md.

use super::World;
use super::recovery::{self, Leaves as RecoveryLeaves};
use crate::cpu_abi::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::render_inputs;
use crate::render_records::{
    NativeAuxiliaryInputs as AuxiliaryInputs, NativePublicationInputs as Inputs,
};
use crate::resolution::rsf_fallout4_resolution_inputs;
use crate::resolution_stages::{
    RSF_FALLOUT4_STAGE_EVALUATED as EVALUATED, RSF_FALLOUT4_STAGE_OUTPUT_READY as OUTPUT_READY,
};
use crate::screen::NativeScreen as Screen;
use crate::{RSF_ERROR_NATIVE_REFUSED, RSF_ERROR_NOT_READY, RSF_OK, image, message};
use rsf_game_sys::d3d11_reduced_abi::{
    RSF_D3D11_AUXILIARY_COLOUR, RSF_D3D11_AUXILIARY_DEPTH_STENCIL,
    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS, rsf_d3d11_auxiliary_surface as Surface,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_FINAL_SCENE,
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RECONFIGURE,
    RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW, rsf_game_render_config, rsf_game_render_pass,
    rsf_result,
};
use rsf_game_sys::{RSF_SCREEN_GAMEPLAY as SCREEN_GAMEPLAY, RSF_SCREEN_UNKNOWN as SCREEN_UNKNOWN};
use std::ffi::{CStr, c_char, c_void};
use std::mem::offset_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::Interface;

// The Effects call inside imagespace whose return is the SR pass key.
const SR_PASS: usize = super::EFFECTS_CALL.0;

const SCREEN_RECEIPTS: u32 = 8;

/// Facts from the same framebuffer acquisition that owns the texture.
#[derive(Default)]
struct Framebuffer {
    context: *mut c_void,
    chain: *mut c_void,
    texture: *mut c_void,
    native_frame: u64,
    window_key: u64,
    extent: [u32; 3],
}

const _: () = {
    assert!(size_of::<CpuIdentity>() == 40);
    assert!(size_of::<Inputs>() == 3 * size_of::<usize>());
    assert!(size_of::<AuxiliaryInputs>() == 56 && offset_of!(AuxiliaryInputs, raster) == 40);
};

/// NativePublicationLeaves, constant for the process. Every leaf acts on the calling thread's
/// World.
#[repr(C)]
pub(super) struct Leaves {
    world: unsafe extern "C" fn(&mut World),
    // The render hooks' shared diagnostic budget.
    diagnostic: unsafe extern "C" fn(*const c_char),
    // Replace the SR input lease with a fresh producer read.
    read_inputs: unsafe extern "C" fn() -> bool,
    inputs: unsafe extern "C" fn(&mut Inputs),
    release_inputs: unsafe extern "C" fn(),
    // The SR input lease names the held recovery colour, depth, motion, Pip-Boy, VATS and
    // refraction resources.
    same_recovery_resources: unsafe extern "C" fn() -> bool,
    depth_domains: unsafe extern "C" fn(*mut rsf_game_render_pass) -> bool,
    // The RECONFIGURE planning call on the input Context; false when it threw.
    plan: unsafe extern "C" fn(*const rsf_game_render_pass) -> bool,
    // 1 when the requested configuration was read, 0 when unbound or refused, -1 on a throw.
    render_config: unsafe extern "C" fn(&mut rsf_game_render_config) -> i32,
    // Keep the colour, depth and motion leases and record the screen basis.
    retain_inputs: unsafe extern "C" fn(*const rsf_game_render_pass),
    // Classify against the screen basis; `first` records the result as the leased screen.
    classify: unsafe extern "C" fn(bool, &mut Screen, &mut Screen),
    notify: unsafe extern "C" fn(*const rsf_game_render_pass, *mut c_void, u32),
    // World pass, Context and chain from the scalar pass and the input lease; inputs published.
    publish_inputs: unsafe extern "C" fn(*const rsf_game_render_pass),
    begin_output: unsafe extern "C" fn(*const rsf_game_render_pass) -> bool,
    report_state: unsafe extern "C" fn(u32, *const rsf_game_render_pass, *mut c_void, bool, i32),
    // The guarded auxiliary or full-output evaluation of the input lease.
    auxiliary_inputs: unsafe extern "C" fn(&mut AuxiliaryInputs) -> bool,
    auxiliary_colour: unsafe extern "C" fn(*mut c_void) -> bool,
    same_texture: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    reconstruct:
        unsafe extern "C" fn(*const rsf_game_render_pass, *const Surface, u32) -> rsf_result,
    end_output: unsafe extern "C" fn() -> bool,
    // Retain the final colour from that lease and open WINDOW.
    publish_window: unsafe extern "C" fn(*mut c_void, *const rsf_game_render_pass),
}

const _: () = assert!(size_of::<Leaves>() == 21 * size_of::<usize>());

impl Leaves {
    fn world(&self) -> World {
        World::read(self.world)
    }
    fn diagnostic(&self, text: &CStr) {
        unsafe { (self.diagnostic)(text.as_ptr()) }
    }
    fn inputs(&self) -> Inputs {
        let mut inputs = Inputs {
            pass: std::ptr::null(),
            context: null_mut(),
            chain: null_mut(),
        };
        unsafe { (self.inputs)(&mut inputs) };
        inputs
    }
    fn classify(&self, first: bool) -> (Screen, Screen) {
        let mut current = Screen::EMPTY;
        let mut leased = Screen::EMPTY;
        unsafe { (self.classify)(first, &mut current, &mut leased) };
        (current, leased)
    }
    fn notify(&self, pass: &rsf_game_render_pass, context: *mut c_void, begin: u32) {
        unsafe { (self.notify)(pass, context, begin) }
    }
    fn report_state(
        &self,
        stage: u32,
        pass: &rsf_game_render_pass,
        context: *mut c_void,
        published: bool,
        result: rsf_result,
    ) {
        unsafe { (self.report_state)(stage, pass, context, published, result) }
    }
}

/// FINISHes the recovery token when the SR step returns, after the input lease is released, as
/// the C++ scope guard did.
struct Finish<'a>(&'a RecoveryLeaves);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        let _ = recovery::finish(self.0);
    }
}

/// Releases the SR input lease when the SR step returns, as the C++ stack lease ended.
struct InputLease<'a>(&'a Leaves);

impl Drop for InputLease<'_> {
    fn drop(&mut self) {
        unsafe { (self.0.release_inputs)() }
    }
}

static SCREEN_ROWS: AtomicU32 = AtomicU32::new(0);
static HUDLESS_STAGES: AtomicU32 = AtomicU32::new(0);

/// Pass identity from the World's CPU source. The admitted ordinary WorldBegin follows the
/// native update/redraw work; this constructor fact does not report a SIM_END or classify
/// gameplay. The FRAME and SUBMISSION passes (scope.rs) share it.
pub(super) fn identify(pass: &mut rsf_game_render_pass, cpu: &CpuIdentity) {
    pass.session_id = cpu.session;
    pass.source_frame_id = cpu.source;
    pass.submission_id = cpu.submission;
    pass.scope_id = cpu.submission;
    pass.flags |=
        RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
    pass.screen = SCREEN_UNKNOWN;
}

/// Classifies the screen against the World's screen basis. The first call of a source records
/// the leased screen and its receipt; every call refuses the World unless the native screen is
/// unrefused gameplay, the source is current, the leased screen is unchanged and FRAME and
/// SUBMISSION are open. The pass carries the screen and NO_FG follows the World refusal.
fn screen_facts(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    pass: &mut rsf_game_render_pass,
    first: bool,
) -> Screen {
    let (facts, leased) = leaves.classify(first);
    if first && SCREEN_ROWS.fetch_add(1, Ordering::Relaxed) < SCREEN_RECEIPTS {
        let text = message(format!(
            "Fallout4 screen facts source={} native={} screen={} facts=0x{:x} refusals=0x{:x} camera={}",
            leaves.world().cpu.source,
            pass.native_frame,
            facts.screen,
            facts.facts,
            facts.refusals,
            facts.camera_index,
        ));
        recovery.report_text(&text);
    }
    // The source and World reads are pure, so evaluating them together keeps the C++ verdict.
    let changed = facts.refusals != 0
        || facts.screen != SCREEN_GAMEPLAY
        || !recovery.same_source()
        || facts.screen != leased.screen
        || facts.facts != leased.facts
        || facts.camera_state != leased.camera_state;
    let world = leaves.world();
    if changed || !world.frame_open || !world.submission_open {
        recovery.refuse();
    }
    pass.screen = facts.screen;
    if leaves.world().refused {
        pass.flags |= RSF_GAME_RENDER_NO_FG;
    } else {
        pass.flags &= !RSF_GAME_RENDER_NO_FG;
    }
    leased
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SrOutcome {
    Refused,
    Published,
    // A Runtime call that the C++ owner let propagate threw.
    Failed,
}

/// The SR seam before the original Effects range, sequenced by imagespace.rs.
pub(super) fn emit_sr(leaves: &Leaves, recovery: &RecoveryLeaves) -> SrOutcome {
    // Seal originals before any temporal reader, CPU attribution or screen gate.
    let sealed = recovery::seal(recovery);
    let _resolve = Finish(recovery);
    if !sealed {
        recovery.refuse();
        return SrOutcome::Refused;
    }
    let world = leaves.world();
    if !super::live()
        || !world.in_imagespace
        || !recovery.same_source()
        || world.inputs
        || world.raster_refused
        || world.auxiliary_open
        || !world.upscale
    {
        return SrOutcome::Refused;
    }
    let _inputs = InputLease(leaves);
    if !unsafe { (leaves.read_inputs)() } {
        return SrOutcome::Refused;
    }
    if recovery::token() != 0 && !unsafe { (leaves.same_recovery_resources)() } {
        return SrOutcome::Refused;
    }
    let lease = leaves.inputs();
    // SAFETY: the input lease always names its pass.
    let mut pass = unsafe { *lease.pass };
    identify(&mut pass, &world.cpu);
    pass.role = RSF_GAME_RENDER_SR;
    pass.pass_key = (image() + SR_PASS) as u64;
    if !rsf_fallout4_resolution_inputs(&pass) {
        leaves.diagnostic(
            c"Fallout4 SR refused: actual producer LTRB crop/camera/output extents disagree.",
        );
        return SrOutcome::Refused;
    }
    let world = leaves.world();
    // SAFETY: the World names its FRAME pass.
    let frame = unsafe { &*world.frame_pass };
    if !world.frame_open
        || pass.native_frame != frame.native_frame
        || pass.view_key != frame.view_key
        || pass.window_key != frame.window_key
        || lease.context != world.context
        || lease.chain != world.chain
    {
        recovery.refuse();
        leaves.diagnostic(
            c"Fallout4 SR refused: native lease differs from the pre-world FRAME/SUBMISSION owner.",
        );
        return SrOutcome::Refused;
    }
    if !world.depth_valid || !unsafe { (leaves.depth_domains)(&mut pass) } {
        recovery.refuse();
        leaves.diagnostic(
            c"Fallout4 temporal publication refused: same-source native prepass depth mapping unavailable; raw mixed depth not substituted.",
        );
        return SrOutcome::Refused;
    }
    let mut planning = pass;
    planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
    if !unsafe { (leaves.plan)(&planning) } {
        return SrOutcome::Refused;
    }
    let mut requested = rsf_game_render_config {
        struct_size: size_of::<rsf_game_render_config>() as u32,
        enabled: 0,
        output_width: 0,
        output_height: 0,
        render_width: 0,
        render_height: 0,
    };
    let configured = unsafe { (leaves.render_config)(&mut requested) };
    if configured < 0 {
        return SrOutcome::Failed;
    }
    let sr_off = configured > 0
        && requested.enabled == 0
        && requested.output_width == pass.camera.output_width
        && requested.output_height == pass.camera.output_height;
    unsafe { (leaves.retain_inputs)(&pass) };
    let mut fg = pass;
    let screen = screen_facts(leaves, recovery, &mut fg, true);
    if screen.refusals != 0 || screen.screen != SCREEN_GAMEPLAY || !recovery.same_source() {
        return SrOutcome::Refused;
    }
    // SR and diagnostic consumers receive the same verified screen fact.
    pass.screen = fg.screen;
    fg.role = RSF_GAME_RENDER_FG_INPUTS;
    leaves.notify(&fg, lease.context, 1);
    leaves.notify(&fg, lease.context, 0);
    if !recovery.same_source() {
        recovery.refuse();
        return SrOutcome::Refused;
    }
    // The host normalizes and retains these contents synchronously before SR. No borrowed
    // resource pointer survives in the scalar identity.
    let scalar = fg.without_resources();
    unsafe { (leaves.publish_inputs)(&scalar) };
    if sr_off {
        // Keep actual FG/native output facts, with no SR/output-state mutation.
        return SrOutcome::Refused;
    }
    if !unsafe { (leaves.begin_output)(&pass) } {
        leaves.report_state(
            OUTPUT_READY,
            &pass,
            lease.context,
            false,
            RSF_ERROR_NOT_READY,
        );
        recovery.refuse();
        return SrOutcome::Refused;
    }
    leaves.report_state(OUTPUT_READY, &pass, lease.context, false, RSF_OK);
    let result = reconstruct(leaves, &pass);
    leaves.report_state(EVALUATED, &pass, lease.context, result == RSF_OK, result);
    if result != RSF_OK {
        // A sealed ticket owns rollback/recovery; retain its output scope until FINISH so
        // spatial resolution is not followed by reduced selectors.
        if recovery::token() == 0 && !unsafe { (leaves.end_output)() } {
            recovery.refuse();
        }
        return SrOutcome::Refused;
    }
    // Published; full output is resolved here only without a token, whose FINISH decides it.
    let full_output = recovery::token() == 0 || recovery.full_output_resolved();
    recovery.resolve(full_output, true);
    leaves.diagnostic(
        c"Fallout4 producer-verified SR publication accepted before imagespace; scoped native output/auxiliary consumers active, FG remains unproven.",
    );
    SrOutcome::Published
}

// Keep the depth mapping on the integer render rectangle. The colour surfaces share the
// producer's fractional viewport and the same camera jitter.
fn reconstruct(leaves: &Leaves, pass: &rsf_game_render_pass) -> rsf_result {
    let reduced = pass.camera.render_width != pass.camera.output_width
        || pass.camera.render_height != pass.camera.output_height
        || pass.render_rect[0] != 0
        || pass.render_rect[1] != 0;
    if !reduced {
        return unsafe { (leaves.reconstruct)(pass, std::ptr::null(), 0) };
    }
    let mut inputs = AuxiliaryInputs {
        textures: [null_mut(); 5],
        raster: [0.0; 4],
    };
    if !unsafe { (leaves.auxiliary_inputs)(&mut inputs) }
        || !unsafe { (leaves.auxiliary_colour)(inputs.textures[1]) }
    {
        return RSF_ERROR_NATIVE_REFUSED;
    }

    let mut surfaces = [Surface::default(); 5];
    let mut count = 0;
    let [x, y, width, height] = inputs.raster;
    let colours = [x, y, x + width, y + height];
    let names = [
        (c"native_depth_expanded", c"native_depth_partitioned"),
        (c"native_motion_expanded", c"native_motion_reduced"),
        (c"native_pipboy_expanded", c"native_pipboy_reduced"),
        (c"native_vats_expanded", c"native_vats_reduced"),
        (c"native_refraction_expanded", c"native_refraction_reduced"),
    ];
    for (slot, (texture, (name, source_name))) in inputs.textures.into_iter().zip(names).enumerate()
    {
        if slot >= 2
            && (!unsafe { (leaves.auxiliary_colour)(texture) }
                || unsafe { (leaves.same_texture)(texture, pass.color_output) }
                || surfaces[..count]
                    .iter()
                    .any(|surface| unsafe { (leaves.same_texture)(texture, surface.texture) }))
        {
            continue;
        }
        let depth = slot == 0;
        surfaces[count] = Surface {
            struct_size: size_of::<Surface>() as u32,
            kind: if depth {
                RSF_D3D11_AUXILIARY_DEPTH_STENCIL
            } else {
                RSF_D3D11_AUXILIARY_COLOUR
            },
            texture,
            source_rect: if depth {
                pass.render_rect.map(|coordinate| coordinate as f32)
            } else {
                colours
            },
            output_rect: pass.output_rect,
            jitter_pixels: pass.camera.jitter_pixels,
            name: name.as_ptr(),
            source_name: source_name.as_ptr(),
            flags: if slot == 3 {
                RSF_D3D11_AUXILIARY_RESTORE_CONTENTS
            } else {
                0
            },
        };
        count += 1;
    }
    if recovery::token() != 0 && count != surfaces.len() {
        return RSF_ERROR_NATIVE_REFUSED;
    }
    unsafe { (leaves.reconstruct)(pass, surfaces.as_ptr(), count as u32) }
}

/// MSVC `%p`: sixteen uppercase hexadecimal digits.
fn pointer(value: usize) -> String {
    format!("{value:016X}")
}

/// The HUDless seam after the native HUDless viewport selection.
pub(super) fn emit_hudless(leaves: &Leaves, recovery: &RecoveryLeaves) {
    emit_hudless_with(leaves, recovery, || {
        let lease = render_inputs::acquire_framebuffer(Some(recovery.report));
        let frame = lease
            .as_ref()
            .map_or_else(Framebuffer::default, |lease| Framebuffer {
                context: lease.context.as_raw(),
                chain: lease.chain.as_raw(),
                texture: lease.texture.as_raw(),
                native_frame: lease.native_frame,
                window_key: lease.window_key,
                extent: crate::d3d11::framebuffer_extent(&lease.texture),
            });
        (lease.is_some(), frame, lease)
    });
}

fn emit_hudless_with<Lease>(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    acquire: impl FnOnce() -> (bool, Framebuffer, Lease),
) {
    let world = leaves.world();
    if !world.in_imagespace || !world.inputs || world.window_open || !recovery.same_source() {
        return;
    }
    // Retain the acquisition through both FINAL_SCENE callbacks and the WINDOW handoff.
    let (acquired, frame, _lease) = acquire();
    // The first refusal of each stage, naming the framebuffer and the retained identities.
    let refuse = |stage: u32, name: &str| {
        let bit = 1u32 << stage;
        if HUDLESS_STAGES.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
            return;
        }
        let world = leaves.world();
        // SAFETY: the World names its passes.
        let (pass, frame_pass, screen_basis) =
            unsafe { (&*world.pass, &*world.frame_pass, &*world.screen_basis) };
        let text = message(format!(
            "Fallout4 HUDless stage={} source={} native={}/{} frame_basis={} screen_basis={} window={}/{} context={}/{} outer_chain={}/{}",
            name,
            world.cpu.source,
            frame.native_frame,
            pass.native_frame,
            frame_pass.native_frame,
            screen_basis.native_frame,
            pointer(frame.window_key as usize),
            pointer(pass.window_key as usize),
            pointer(frame.context as usize),
            pointer(world.context as usize),
            pointer(frame.chain as usize),
            pointer(world.chain as usize),
        ));
        recovery.report_text(&text);
    };
    // A failed logical-0 read is not evidence that any identity comparison failed.
    if !acquired {
        refuse(0, "framebuffer-read");
        return;
    }
    let world = leaves.world();
    // SAFETY: the World names its pass.
    let pass = unsafe { *world.pass };
    if frame.native_frame != pass.native_frame {
        refuse(1, "native-frame");
        return;
    }
    if frame.window_key != pass.window_key {
        refuse(2, "window");
        return;
    }
    if frame.context != world.context {
        refuse(3, "context");
        return;
    }
    if frame.chain != world.chain {
        refuse(4, "outer-chain");
        return;
    }
    let [width, height, samples] = frame.extent;
    if width != pass.camera.output_width || height != pass.camera.output_height || samples != 1 {
        refuse(5, "output-extent");
        return;
    }
    let mut last = pass;
    screen_facts(leaves, recovery, &mut last, false);
    let world = leaves.world();
    last.role = RSF_GAME_RENDER_FINAL_SCENE;
    last.scene_surface = frame.texture;
    last.swapchain = world.chain;
    leaves.notify(&last, world.context, 1);
    leaves.notify(&last, world.context, 0);
    let mut window = last;
    window.role = RSF_GAME_RENDER_WINDOW;
    window.swapchain = world.chain;
    window.scene_surface = null_mut();
    unsafe { (leaves.publish_window)(frame.texture, &window) };
    let world = leaves.world();
    // SAFETY: the World names its pass, now the open WINDOW.
    leaves.notify(unsafe { &*world.pass }, world.context, 1);
}

/// The screen verdict on a World pass, used again before the outer Present.
///
/// # Safety
/// A nonnull `pass` is a World pass of the calling thread.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_render_screen_facts(
    leaves: &Leaves,
    recovery: &RecoveryLeaves,
    pass: *mut rsf_game_render_pass,
    first: bool,
) {
    // Work on a copy: the leaves read the World while the verdict is formed.
    let Some(mut copy) = (unsafe { pass.as_ref() }).copied() else {
        return;
    };
    screen_facts(leaves, recovery, &mut copy, first);
    unsafe { pass.write(copy) };
}

#[cfg(test)]
mod tests;
