// SPDX-License-Identifier: MIT
//! Godot D3D12 SR context and submission owner formerly held by `runtime.cpp`: the engine's
//! device and queue identity, the SR context plan (requested provider, Auto resolved by the
//! Runtime, spatial mapped to the normalising context, the fallback to backend 0, the planned
//! input extent and the creation result) with its retirement and recreation rules, the three
//! command slots and their fence values, the pending submissions `rsfg_submitted` matches, the
//! FG provider switch request, screen gating, sharpening defaults, the refusal diagnostics budget,
//! the current render frame and the pass identity Present reuses.
//!
//! `runtime.cpp` keeps the leaves and decides nothing: the device, queue and fence references,
//! the fence wait and signal, the DXGI vendor lookup behind the Runtime's Auto provider policy
//! (`rsf_gpu_default_sr_backend`, `rsf_fg_default_backend`), `rsf_sr12_*`, `rsf_fg12_*`, the
//! `rsf_d3d11_present_*` provider calls, frame capture and the diagnostics text. The context
//! handle is held here and lent to each leaf.
//!
//! These paths run on Godot's render thread, as before. The ledger lock is never held across a
//! leaf except the fence signal, which calls no owner and keeps fence values in submission order.
use crate::control::Control;
use crate::platform::{Platform, Settings, Sharpening, SrCreate, SrEvaluate, SrFrame};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW, RSF_SCREEN_FLIGHT,
    RSF_SCREEN_MENU, RSF_SCREEN_UNKNOWN, backend_result, rsf_game_render_pass,
};
use std::ffi::{c_int, c_void};
use std::sync::{Mutex, MutexGuard};

const SR_NONE: u32 = 0;
const SR_SPATIAL: u32 = 6;
const SR_AUTO: u32 = 7;
const FG_BACKEND_AUTO: u32 = 7;
/// `rsf_sr12_evaluate` command slots 0..2.
const SLOTS: u32 = 3;
/// Refused passes whose inputs are logged.
const DIAGNOSTIC_LIMIT: u32 = 4;

/// The pass identity Present reuses. The engine's borrowed texture handles are cleared.
#[derive(Clone, Copy)]
struct Rendered(rsf_game_render_pass);

// SAFETY: the texture handles are cleared before the copy is stored. The remaining pointer
// fields are identity values copied from the engine's pass; this crate never dereferences them
// and hands the record only to the Present leaf, as `runtime.cpp` did with its global copy.
unsafe impl Send for Rendered {}

impl Rendered {
    fn of(pass: &rsf_game_render_pass) -> Self {
        let mut copy = *pass;
        for handle in [
            &mut copy.color_input,
            &mut copy.color_output,
            &mut copy.depth,
            &mut copy.motion,
            &mut copy.exposure,
            &mut copy.color_output_readable,
            &mut copy.ui_input,
            &mut copy.swapchain,
            &mut copy.scene_surface,
            &mut copy.sampled_texture,
        ] {
            *handle = std::ptr::null_mut();
        }
        Self(copy)
    }
}

/// A live `rsf_sr12` context and the request it was created for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Context {
    /// The Runtime handle, kept as an address.
    handle: usize,
    /// The provider that was created; zero normalises inputs only.
    backend: u32,
    generation: u64,
    output: (u32, u32),
}

impl Context {
    /// A settings generation change, a new output extent, or, for the normalising context, an
    /// input extent other than its plan drains and recreates the context.
    fn stale(
        &self,
        generation: u64,
        output: (u32, u32),
        render: (u32, u32),
        plan: (u32, u32),
    ) -> bool {
        self.generation != generation
            || self.output != output
            || (self.backend == SR_NONE && render != plan)
    }
}

/// A command list recorded for one slot, waiting for its submission.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    commands: usize,
    slot: u32,
    session: u64,
    frame: u64,
    /// The HUD-less colour record; its submission is reported to FG.
    hudless: bool,
    /// The engine reported the list on the bound queue, even if the signal refused.
    submitted: bool,
}

struct Ledger {
    /// The engine's first device and queue, as addresses.
    bound: Option<(usize, usize)>,
    context: Option<Context>,
    /// `rsf_sr12_plan` of the last context made; kept after retirement, as before.
    plan: (u32, u32),
    /// The first creation result of the last context request, before any fallback.
    context_result: i32,
    /// The last fence value signalled.
    submitted: u64,
    /// The fence value each slot's last submission signalled.
    slots: [u64; SLOTS as usize],
    next_slot: u32,
    pending: Vec<Pending>,
    current_frame: u64,
    rendered: Option<Rendered>,
    attempted_sr: Option<(u64, u64)>,
    sr_slot: Option<((u64, u64), u32)>,
    diagnostics: u32,
}

pub(crate) struct D3d12 {
    ledger: Mutex<Ledger>,
}

/// FG runs only in requested gameplay on the flight screen; every other pass is a menu pass.
fn screen(settings: &Settings, screen: u32) -> u32 {
    if settings.gameplay != 0 && screen == RSF_SCREEN_FLIGHT {
        RSF_SCREEN_FLIGHT
    } else {
        RSF_SCREEN_MENU
    }
}

impl D3d12 {
    pub(crate) fn new() -> Self {
        Self {
            ledger: Mutex::new(Ledger {
                bound: None,
                context: None,
                plan: (0, 0),
                context_result: backend_result::OK,
                submitted: 0,
                slots: [0; SLOTS as usize],
                next_slot: 0,
                pending: Vec::new(),
                current_frame: 0,
                rendered: None,
                attempted_sr: None,
                sr_slot: None,
                diagnostics: 0,
            }),
        }
    }

    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        lock_or_recover(&self.ledger)
    }

    /// One `rsfg_render` pass. The caller checked the session, the arguments, the pass size and
    /// its depth domains.
    pub(crate) fn render(
        &self,
        control: &Control,
        device: *mut c_void,
        queue: *mut c_void,
        commands: *mut c_void,
        pass: &rsf_game_render_pass,
    ) -> c_int {
        let identity = (device as usize, queue as usize);
        {
            let mut ledger = self.ledger();
            if pass.role == RSF_GAME_RENDER_SR {
                ledger.attempted_sr = Some((pass.session_id, pass.source_frame_id));
                ledger.sr_slot = None;
            }
            match ledger.bound {
                // Binding creates the fence and calls no owner.
                None if Platform.d3d12_bind(device, queue) => ledger.bound = Some(identity),
                Some(bound) if bound == identity => {}
                _ => return 0,
            }
        }
        if pass.role == RSF_GAME_RENDER_SR {
            Platform.capture_sr_begin(pass);
        }
        if pass.role == RSF_GAME_RENDER_FINAL_SCENE {
            return self.final_scene(commands, pass);
        }
        let (settings, generation) = control.snapshot();
        let fg = match settings.fg_backend {
            FG_BACKEND_AUTO => Platform.fg_default_backend(),
            backend => backend,
        };
        if let Some(current) = Platform.present_owner()
            && fg != current
        {
            Platform.present_request(fg);
        }
        let camera = &pass.camera;
        let output = (camera.output_width, camera.output_height);
        let render = (camera.render_width, camera.render_height);
        let retire = {
            let ledger = self.ledger();
            let stale = ledger
                .context
                .filter(|context| context.stale(generation, output, render, ledger.plan));
            if stale.is_some() && !ledger.pending.is_empty() {
                return 0;
            }
            stale.map(|context| (context.handle, ledger.submitted))
        };
        if let Some((handle, submitted)) = retire {
            // All submitted work drains before the context is destroyed.
            if !Platform.d3d12_wait(submitted) {
                return 0;
            }
            Platform.sr_destroy(handle as *mut c_void);
            self.ledger().context = None;
        }
        let existing = self.ledger().context;
        let Some(context) =
            existing.or_else(|| self.create(control, &settings, generation, output, render))
        else {
            return 0;
        };
        let Some(slot) = self.acquire_slot() else {
            return 0;
        };
        let request = SrEvaluate {
            slot,
            screen: screen(&settings, pass.screen),
            sharpening: sharpening(&settings),
        };
        let evaluated =
            Platform.sr_evaluate(context.handle as *mut c_void, commands, pass, &request);
        let (plan, diagnose) = {
            let mut ledger = self.ledger();
            let diagnose = evaluated == backend_result::INVALID_ARGUMENT
                && ledger.diagnostics < DIAGNOSTIC_LIMIT;
            if diagnose {
                ledger.diagnostics += 1;
            }
            (ledger.plan, diagnose)
        };
        if diagnose {
            Platform.sr_refusal_log(pass, plan);
        }
        // The driver submission callback signals this pending slot after recording is submitted.
        let context_result = {
            let mut ledger = self.ledger();
            ledger.pending.push(Pending {
                commands: commands as usize,
                slot,
                session: pass.session_id,
                frame: pass.source_frame_id,
                hudless: false,
                submitted: false,
            });
            if evaluated == backend_result::OK {
                ledger.rendered = Some(Rendered::of(pass));
                if pass.role == RSF_GAME_RENDER_SR {
                    ledger.sr_slot = Some(((pass.session_id, pass.source_frame_id), slot));
                }
            }
            ledger.context_result
        };
        control.sr_frame(&SrFrame {
            requested_sr: settings.sr_backend,
            context_backend: context.backend,
            context_result,
            evaluated,
            render_width: plan.0,
            render_height: plan.1,
            output_width: context.output.0,
            output_height: context.output.1,
        });
        c_int::from(evaluated == backend_result::OK)
    }

    /// Reuse descriptors only after the slot's last submission completed.
    fn acquire_slot(&self) -> Option<u32> {
        let (slot, fence) = {
            let ledger = self.ledger();
            let slot = ledger.next_slot % SLOTS;
            if ledger.pending.iter().any(|item| item.slot == slot)
                || ledger.sr_slot.is_some_and(|(_, acquired)| acquired == slot)
            {
                return None;
            }
            (slot, ledger.slots[slot as usize])
        };
        if !Platform.d3d12_wait(fence) {
            return None;
        }
        self.ledger().next_slot = (slot + 1) % SLOTS;
        Some(slot)
    }

    /// The HUD-less record can share this source's admitted SR slot while its lists are pending
    /// on the same queue. After submission it must wait for that slot's completion fence.
    fn final_scene(&self, commands: *mut c_void, pass: &rsf_game_render_pass) -> c_int {
        let source = (pass.session_id, pass.source_frame_id);
        let (attempted, admitted) = {
            let mut ledger = self.ledger();
            let attempted = ledger.attempted_sr == Some(source);
            let admitted = ledger.sr_slot.take().filter(|(key, _)| *key == source);
            (attempted, admitted)
        };
        let slot = if let Some((_, slot)) = admitted {
            let fence = {
                let ledger = self.ledger();
                let mut pending = ledger.pending.iter().filter(|item| item.slot == slot);
                if let Some(item) = pending.next() {
                    if item.submitted
                        || (item.session, item.frame) != source
                        || pending.next().is_some()
                    {
                        return 0;
                    }
                    None
                } else {
                    Some(ledger.slots[slot as usize])
                }
            };
            if fence.is_some_and(|fence| !Platform.d3d12_wait(fence)) {
                return 0;
            }
            slot
        } else {
            if attempted {
                return 0;
            }
            let Some(slot) = self.acquire_slot() else {
                return 0;
            };
            slot
        };
        Platform.final_scene(commands, pass, slot);
        let mut ledger = self.ledger();
        ledger.pending.push(Pending {
            commands: commands as usize,
            slot,
            session: pass.session_id,
            frame: pass.source_frame_id,
            hudless: true,
            submitted: false,
        });
        if !attempted {
            ledger.rendered = Some(Rendered::of(pass));
        }
        1
    }

    /// A new context for the current request. A provider that cannot be created falls back to
    /// the normalising context; the first result is the one reported.
    fn create(
        &self,
        control: &Control,
        settings: &Settings,
        generation: u64,
        output: (u32, u32),
        render: (u32, u32),
    ) -> Option<Context> {
        let paths = control.paths()?;
        let mut backend = settings.sr_backend;
        if backend == SR_AUTO {
            backend = Platform.sr_default_backend();
        }
        // Godot evaluates FSR1 before the callback; the context only normalises its inputs.
        if backend == SR_SPATIAL {
            backend = SR_NONE;
        }
        let mut request = SrCreate {
            backend,
            quality: settings.quality,
            dlss_preset: settings.dlss_preset,
            output_width: output.0,
            output_height: output.1,
            render_width: render.0,
            render_height: render.1,
            paths,
        };
        let plan = self.ledger().plan;
        let mut created = Platform.sr_create(&request, plan)?;
        self.ledger().context_result = created.result;
        if created.handle.is_null() && backend != SR_NONE {
            request.backend = SR_NONE;
            created =
                Platform.sr_create(&request, (created.desired_width, created.desired_height))?;
            backend = SR_NONE;
        }
        if created.handle.is_null() {
            return None;
        }
        let context = Context {
            handle: created.handle as usize,
            backend,
            generation,
            output,
        };
        let mut ledger = self.ledger();
        ledger.plan = (created.desired_width, created.desired_height);
        ledger.context = Some(context);
        Some(context)
    }

    /// `rsfg_submitted`: one engine submission on `queue`. When it carries pending command
    /// lists, the fence is signalled once for all of them and each slot takes that value.
    pub(crate) fn submitted(&self, queue: *mut c_void, lists: &[usize]) {
        let hudless = {
            let mut ledger = self.ledger();
            if ledger
                .bound
                .is_none_or(|(_, bound)| bound != queue as usize)
            {
                return;
            }
            for item in &mut ledger.pending {
                item.submitted |= lists.contains(&item.commands);
            }
            if !ledger.pending.iter().any(|item| item.submitted) {
                return;
            }
            // A later signal on this queue also covers previously reported lists.
            let value = ledger.submitted.wrapping_add(1);
            if !Platform.d3d12_signal(value) {
                return;
            }
            ledger.submitted = value;
            let Ledger { pending, slots, .. } = &mut *ledger;
            let mut hudless = Vec::new();
            pending.retain(|item| {
                if !item.submitted {
                    return true;
                }
                if let Some(fence) = slots.get_mut(item.slot as usize) {
                    *fence = value;
                }
                // HUD-less colour depends on the SR output in the engine graph. Its actual
                // submission establishes both captures; recording completion establishes neither.
                if item.hudless {
                    hudless.push((item.session, item.frame));
                }
                false
            });
            hudless
        };
        for (session, frame) in hudless {
            Platform.fg_submitted(session, frame);
        }
    }

    /// `rsfg_render_begin` on a booted session.
    pub(crate) fn render_begin(&self, control: &Control, frame: u64) {
        {
            let mut ledger = self.ledger();
            ledger.current_frame = frame;
            ledger.rendered = None;
            ledger.attempted_sr = None;
            ledger.sr_slot = None;
        }
        if frame != 0 {
            Platform.render_begin(control.session_id(), frame);
        }
    }

    /// `rsfg_present` on a booted session: the current frame's rendered identity, or a bare
    /// identity and a missed SR report when no pass rendered it.
    pub(crate) fn present(&self, control: &Control, chain: *mut c_void) {
        let (current, rendered) = {
            let ledger = self.ledger();
            (ledger.current_frame, ledger.rendered)
        };
        if chain.is_null() || current == 0 {
            return;
        }
        let session = control.session_id();
        let mut pass = if let Some(Rendered(pass)) = rendered
            && pass.source_frame_id == current
            && pass.session_id == session
        {
            pass
        } else {
            control.sr_missed();
            let mut pass = rsf_game_render_pass::default();
            pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
            pass.session_id = session;
            pass.source_frame_id = current;
            pass.native_frame = current;
            pass.screen = RSF_SCREEN_UNKNOWN;
            pass
        };
        pass.role = RSF_GAME_RENDER_WINDOW;
        pass.swapchain = chain;
        Platform.present_window(&pass);
    }

    /// The D3D12 branch of `rsfg_shutdown`: drain every submission, end the FG session, then
    /// flush capture, destroy the context and cancel the session's frame dump. A refusal at any
    /// step keeps the context.
    pub(crate) fn shutdown(&self, control: &Control) -> c_int {
        let session = control.session_id();
        let (submitted, handle) = {
            let ledger = self.ledger();
            if !ledger.pending.is_empty() {
                return 0;
            }
            (
                ledger.submitted,
                ledger.context.map_or(0, |context| context.handle),
            )
        };
        if !Platform.d3d12_wait(submitted) || !Platform.fg_end_session(session) {
            return 0;
        }
        Platform.d3d12_release(handle as *mut c_void, session);
        self.ledger().context = None;
        1
    }
}

/// The requested sharpening, or the Runtime defaults when none was requested. The Vulkan SR
/// owner (`vulkan/sr.rs`) applies the same rule.
pub(crate) fn sharpening(settings: &Settings) -> Sharpening {
    if settings.sharpening.struct_size != 0 {
        settings.sharpening
    } else {
        Platform.sharpening_defaults()
    }
}

#[cfg(test)]
mod tests;
