// SPDX-License-Identifier: MIT
//! The native renderer transaction behind the GameSDK lifecycle (lifecycle.rs), formerly the
//! exported prepare/start/is_active/quiesce/stop of native_renderer.cpp, and the MinHook records
//! of every detour. Admission and the hook table are sites.rs, the engine patches patches.rs, the
//! TrueSky restore sky.rs and the hook body counts guards.rs.
//!
//! Two rules differ from the former C++ so that a partial activation and a retried stop are safe:
//! - A failed installation disables every hook it created, and removes them only once no hook
//!   body is running. While one is, the hooks stay created with their trampolines, so a body
//!   that has not yet called its original still reaches it, and the renderer, its options
//!   callbacks and the installed pointer stay reachable in the pinned module. The former rollback
//!   removed the hooks, freeing the trampolines, before it looked at the guards.
//! - Retirement disables and removes the hooks that are created, which a removal clears, instead
//!   of every bound target, so a retried stop or a stop after a partial activation removes
//!   exactly what is still owned.
use super::sky::restore_truesky;
use super::{
    HOOKS, INSTALLED, Renderer, SITES, drain_retired, game_module, hook, log_to, native,
    rsf_ac7_render_scopes_destroy, rsf_ac7_render_scopes_quiesce,
};
use super::{evidence, frame, graph, particles, shaders, sky, slate, translucency, views, widgets};
use crate::guards;
use crate::patches;
use crate::sites::{self, HOOK_SITES};
use crate::sys::{rsf_ac7_native_renderer, rsf_ac7_native_renderer_options};
use rsf_minhook_sys::{MH_CreateHook, MH_DisableHook, MH_EnableHook, MH_Initialize, MH_RemoveHook};
use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
};
use windows::Win32::System::WindowsProgramming::GetPrivateProfileIntW;
use windows::Win32::UI::WindowsAndMessaging::{HHOOK, UnhookWindowsHookEx};
use windows::core::{PCWSTR, w};

const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;
const MH_ERROR_NOT_CREATED: i32 = 4;
const MH_ERROR_DISABLED: i32 = 6;

/// Queued render-scope capacity of the native pass controller.
const PENDING_CAPACITY: u32 = 256;
const DIAGNOSTICS_VARIABLE: &str = "RSF_AC7_PIXEL_DIAGNOSTICS";

const PINNED: &str =
    "AC7 native preparation failed during a forwarding call; inactive module pinned until exit";
const DIAGNOSTICS: &str = "AC7 native renderer: pixel uniform binding diagnostics enabled";
const PREPARED: &str = "AC7 native renderer prepared: owned view sizing, pre-tonemap SR graph node, downstream engine descriptors, owned widget raster targets and RHI leases; activation pending";

/// The lifecycle's handle of a renderer.
pub(crate) type Handle = *mut rsf_ac7_native_renderer;

/// Attaches each detour to its site. Keyed by hook, so the order of this list carries no meaning.
fn bind_detours() {
    use hook::*;
    let bindings: [(u32, *mut c_void); HOOKS] = [
        (
            PROCESS_TONEMAP,
            graph::hooked_process::<PROCESS_TONEMAP> as *mut c_void,
        ),
        (
            PROCESS_AA,
            graph::hooked_process::<PROCESS_AA> as *mut c_void,
        ),
        (
            PROCESS_MATERIAL,
            graph::hooked_process::<PROCESS_MATERIAL> as *mut c_void,
        ),
        (
            PROCESS_HUD,
            graph::hooked_process::<PROCESS_HUD> as *mut c_void,
        ),
        (
            PROCESS_COMPOSITE,
            graph::hooked_process::<PROCESS_COMPOSITE> as *mut c_void,
        ),
        (
            PROCESS_OUTPUT,
            graph::hooked_process::<PROCESS_OUTPUT> as *mut c_void,
        ),
        (
            DESCRIPTOR_TONEMAP,
            graph::hooked_descriptor::<PROCESS_TONEMAP> as *mut c_void,
        ),
        (
            DESCRIPTOR_AA,
            graph::hooked_descriptor::<PROCESS_AA> as *mut c_void,
        ),
        (
            DESCRIPTOR_MATERIAL,
            graph::hooked_descriptor::<PROCESS_MATERIAL> as *mut c_void,
        ),
        (
            DESCRIPTOR_HUD,
            graph::hooked_descriptor::<PROCESS_HUD> as *mut c_void,
        ),
        (
            DESCRIPTOR_COMPOSITE,
            graph::hooked_descriptor::<PROCESS_COMPOSITE> as *mut c_void,
        ),
        (
            DESCRIPTOR_OUTPUT,
            graph::hooked_descriptor::<PROCESS_OUTPUT> as *mut c_void,
        ),
        (POSTPROCESS, graph::hooked_postprocess as *mut c_void),
        (CONTEXT, graph::hooked_context as *mut c_void),
        (VISIBILITY, views::hooked_visibility as *mut c_void),
        (CONSTRUCT, views::hooked_construct as *mut c_void),
        (WIDGET_QUEUE, widgets::hooked_widget_queue as *mut c_void),
        (
            WIDGET_TARGETS,
            widgets::hooked_widget_targets as *mut c_void,
        ),
        (WIDGET_DRAW, widgets::hooked_widget_draw as *mut c_void),
        (ENGINE_TICK, frame::hooked_engine_tick as *mut c_void),
        (
            RENDERER_RETIRE,
            frame::hooked_renderer_retire as *mut c_void,
        ),
        (POLL_INPUT, frame::hooked_poll_input as *mut c_void),
        (
            GAME_INSTANCE_INIT,
            widgets::hooked_game_instance_init as *mut c_void,
        ),
        (TARGET_INIT, widgets::hooked_target_init as *mut c_void),
        (
            SIMULATION_A,
            frame::hooked_simulation::<SIMULATION_A> as *mut c_void,
        ),
        (
            SIMULATION_B,
            frame::hooked_simulation::<SIMULATION_B> as *mut c_void,
        ),
        (REDRAW, frame::hooked_redraw as *mut c_void),
        (VIEWPORT_DRAW, frame::hooked_viewport_draw as *mut c_void),
        (SLATE_PRIVATE, slate::hooked_slate_private as *mut c_void),
        (SLATE_ALLOCATE, slate::hooked_slate_allocate as *mut c_void),
        (SLATE_TASK, slate::hooked_slate_task as *mut c_void),
        (SLATE_WINDOW, slate::hooked_slate_window as *mut c_void),
        (SLATE_TEXTURE, slate::hooked_slate_texture as *mut c_void),
        (
            TRANSLUCENCY_SIZE,
            translucency::hooked_translucency_size as *mut c_void,
        ),
        (
            UNMODIFIED_BEGIN,
            translucency::hooked_unmodified_begin as *mut c_void,
        ),
        (
            UNMODIFIED_RESOLVE,
            translucency::hooked_unmodified_resolve as *mut c_void,
        ),
        (
            TRANSLUCENCY_RENDER,
            translucency::hooked_translucency_render as *mut c_void,
        ),
        (
            PIXEL_VIEW_UNIFORM,
            evidence::hooked_pixel_view_uniform as *mut c_void,
        ),
        (
            RHI_PIXEL_UNIFORM,
            evidence::hooked_rhi_pixel_uniform as *mut c_void,
        ),
        (
            RHI_PIXEL_TABLES,
            evidence::hooked_rhi_pixel_tables as *mut c_void,
        ),
        (
            PIXEL_ENQUEUE_A,
            evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_A> as *mut c_void,
        ),
        (
            PIXEL_ENQUEUE_B,
            evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_B> as *mut c_void,
        ),
        (
            PIXEL_ENQUEUE_C,
            evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_C> as *mut c_void,
        ),
        (
            PIXEL_ENQUEUE_D,
            evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_D> as *mut c_void,
        ),
        (SKY_PROJECTION, sky::hooked_sky_projection as *mut c_void),
        (SKY_RENDER, sky::hooked_sky_render as *mut c_void),
        (
            TEMPORAL_SAMPLE_INDEX,
            views::hooked_temporal_sample_index as *mut c_void,
        ),
        (
            SCENE_COLOUR_FORMAT,
            translucency::hooked_scene_colour_format as *mut c_void,
        ),
        (RENDER_FAMILY, frame::hooked_render_family as *mut c_void),
        (
            FRAME_TASK_CONSTRUCT,
            frame::hooked_frame_task_construct as *mut c_void,
        ),
        (
            FRAME_TASK_EXECUTE,
            frame::hooked_frame_task_execute as *mut c_void,
        ),
        (
            RHI_FRAME_BEGIN,
            frame::hooked_rhi_frame_begin as *mut c_void,
        ),
        (RHI_FRAME_END, frame::hooked_rhi_frame_end as *mut c_void),
        (FRAME_SYNC, frame::hooked_frame_sync as *mut c_void),
        (PUMP_MESSAGES, frame::hooked_pump_messages as *mut c_void),
        (ENGINE_PACING, frame::hooked_engine_pacing as *mut c_void),
        (
            CREATE_PIXEL_SHADER,
            shaders::hooked_create_pixel_shader as *mut c_void,
        ),
        (BASE_PASS, translucency::hooked_base_pass as *mut c_void),
        (
            TRANSLUCENCY_MESH,
            translucency::hooked_translucency_mesh as *mut c_void,
        ),
        (
            MESH_PARTICLE_PARAMETERS,
            translucency::hooked_mesh_particle_parameters as *mut c_void,
        ),
        (
            BEAM_NO_NOISE,
            particles::hooked_beam_fill::<BEAM_NO_NOISE> as *mut c_void,
        ),
        (
            BEAM_NOISE,
            particles::hooked_beam_fill::<BEAM_NOISE> as *mut c_void,
        ),
        (
            BEAM_INTERPOLATED_NOISE,
            particles::hooked_beam_fill::<BEAM_INTERPOLATED_NOISE> as *mut c_void,
        ),
        (
            RIBBON_FILL,
            particles::hooked_beam_fill::<RIBBON_FILL> as *mut c_void,
        ),
        (
            ANIM_TRAIL_FILL,
            particles::hooked_beam_fill::<ANIM_TRAIL_FILL> as *mut c_void,
        ),
    ];
    for (hook, detour) in bindings {
        SITES[hook as usize].detour.store(detour, Ordering::Relaxed);
    }
}

/// The bound target of `hook`; null for a hook that was not bound (the evidence hooks without
/// diagnostics).
fn bound_target(hook: usize) -> Option<*mut c_void> {
    Some(SITES[hook].target.load(Ordering::Relaxed)).filter(|target| !target.is_null())
}

fn bound_hooks() -> impl Iterator<Item = usize> {
    (0..HOOKS).filter(|hook| bound_target(*hook).is_some())
}

/// MH_CreateHook writes the trampoline only on success; a removal clears it again.
fn created_hooks() -> impl Iterator<Item = (usize, *mut c_void)> {
    (0..HOOKS).filter_map(|hook| {
        bound_target(hook)
            .filter(|_| !SITES[hook].original.load(Ordering::Relaxed).is_null())
            .map(|target| (hook, target))
    })
}

/// All or nothing, as ac7_hooks.h: creates every bound hook in Hook order, then enables them in
/// the same order. Stops at the first failure and leaves the rollback to the caller.
fn install_hooks() -> bool {
    bound_hooks().all(|hook| {
        let site = &SITES[hook];
        let created = unsafe {
            MH_CreateHook(
                site.target.load(Ordering::Relaxed),
                site.detour.load(Ordering::Relaxed),
                site.original.as_ptr(),
            )
        };
        created == MH_OK
    }) && bound_hooks()
        .all(|hook| unsafe { MH_EnableHook(SITES[hook].target.load(Ordering::Relaxed)) } == MH_OK)
}

/// A null target is a hook that was never installed; MH_DisableHook(NULL) would disable them all.
fn disable_created() -> bool {
    let mut disabled = true;
    for (_, target) in created_hooks() {
        let result = unsafe { MH_DisableHook(target) };
        disabled &= matches!(result, MH_OK | MH_ERROR_DISABLED | MH_ERROR_NOT_CREATED);
    }
    disabled
}

fn remove_created() -> bool {
    let mut removed = true;
    for (hook, target) in created_hooks() {
        let result = unsafe { MH_RemoveHook(target) };
        if result == MH_OK || result == MH_ERROR_NOT_CREATED {
            SITES[hook].original.store(null_mut(), Ordering::Relaxed);
        } else {
            removed = false;
        }
    }
    removed
}

fn pin() {
    let mut pinned = HMODULE::default();
    let address = pin as *const u16;
    let _ = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            PCWSTR(address),
            &mut pinned,
        )
    };
}

fn scopes_create(owner: &Renderer) -> bool {
    let options = &owner.options;
    let Some(create) = native().and_then(|calls| calls.scopes_create) else {
        return false;
    };
    let mut scopes = owner.scopes.load(Ordering::Relaxed);
    let created = unsafe {
        create(
            options.session_id,
            PENDING_CAPACITY,
            options.on_state,
            options.on_stage,
            options.user,
            &mut scopes,
        )
    } != 0;
    owner.scopes.store(scopes, Ordering::SeqCst);
    created
}

fn scopes_destroy(owner: &Renderer) -> bool {
    if unsafe { rsf_ac7_render_scopes_destroy(owner.scopes()) } == 0 {
        return false;
    }
    owner.scopes.store(null_mut(), Ordering::SeqCst);
    true
}

/// Releases a renderer that was never published. Scope destruction refuses only queued markers,
/// which an inactive renderer has none of; the former code ignored its result as well.
fn discard(renderer: *mut Renderer) {
    let owner = unsafe { &*renderer };
    unsafe { rsf_ac7_render_scopes_quiesce(owner.scopes()) };
    scopes_destroy(owner);
    drop(unsafe { Box::from_raw(renderer) });
}

/// RSF_AC7_PIXEL_DIAGNOSTICS as the former GetEnvironmentVariableA read it into eight bytes: a
/// value of one to seven characters that does not start with '0'.
fn environment_flag(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| {
        let text = value.as_encoded_bytes();
        text.len() < 8 && text.first().is_some_and(|first| *first != b'0')
    })
}

/// Whether AC7.ini's [Startup] RSF_AC7_PIXEL_DIAGNOSTICS, defaulting to the environment, asks for
/// the pixel uniform binding diagnostic.
fn pixel_diagnostics() -> bool {
    let fallback = environment_flag(std::env::var_os(DIAGNOSTICS_VARIABLE).as_deref());
    let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty()) else {
        return fallback;
    };
    let path = std::path::PathBuf::from(local)
        .join("ReScaleFrame")
        .join("AC7.ini");
    let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        GetPrivateProfileIntW(
            w!("Startup"),
            w!("RSF_AC7_PIXEL_DIAGNOSTICS"),
            i32::from(fallback),
            PCWSTR(path.as_ptr()),
        ) != 0
    }
}

fn installed(handle: Handle) -> bool {
    !handle.is_null()
        && INSTALLED
            .load(Ordering::SeqCst)
            .cast::<rsf_ac7_native_renderer>()
            == handle
}

/// Prepares a renderer for the host options lifecycle.rs built from validated GameSDK services.
/// Refused while another renderer is installed. A renderer with `false` is a pinned inactive
/// renderer that stop retires later.
pub(crate) fn prepare(options: &rsf_ac7_native_renderer_options) -> (Handle, bool) {
    let refused = (null_mut(), false);
    if !INSTALLED.load(Ordering::SeqCst).is_null() {
        return refused;
    }
    let base = game_module();
    // Exact-build admission before anything is allocated or hooked.
    if !sites::admit(base, |message| log_to(options, message)) {
        return refused;
    }
    let Some(renderer) = Renderer::create(options) else {
        return refused;
    };
    let renderer = Box::into_raw(renderer);
    let owner = unsafe { &*renderer };
    if !scopes_create(owner) {
        drop(unsafe { Box::from_raw(renderer) });
        return refused;
    }
    let init = unsafe { MH_Initialize() };
    if init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED {
        discard(renderer);
        return refused;
    }
    bind_detours();
    // The evidence hooks run for every pixel uniform bind. Their sites were verified above, but
    // they are hooked only when the diagnostic is asked for (sites::wanted).
    let diagnostics = pixel_diagnostics();
    for (hook, site) in HOOK_SITES.iter().enumerate() {
        let target = if sites::wanted(hook as u32, diagnostics) {
            base.wrapping_add(site.rva as usize)
        } else {
            null_mut()
        };
        SITES[hook].target.store(target.cast(), Ordering::Relaxed);
    }
    INSTALLED.store(renderer, Ordering::SeqCst);
    if !install_hooks() {
        let disabled = disable_created();
        owner.deactivate();
        if !disabled || guards::held(true) != 0 || !remove_created() {
            // Keep the renderer and any remaining hooks until stop proves retirement.
            pin();
            owner.quiescing.store(true, Ordering::SeqCst);
            unsafe { rsf_ac7_render_scopes_quiesce(owner.scopes()) };
            owner.log_text(PINNED);
            return (owner.handle(), false);
        }
        INSTALLED.store(null_mut(), Ordering::SeqCst);
        discard(renderer);
        return refused;
    }
    if diagnostics {
        owner.log_text(DIAGNOSTICS);
    }
    owner.log_text(PREPARED);
    (owner.handle(), true)
}

/// Scene precision, then temporal velocity (patches.rs). A refusal keeps what was applied owned
/// until stop restores it.
pub(crate) fn start(handle: Handle) -> bool {
    if !installed(handle) {
        return false;
    }
    let owner = unsafe { Renderer::from_handle(handle) };
    if owner.quiescing() {
        return false;
    }
    // SAFETY: only the GameSDK lifecycle changes the patch record, one transition at a time.
    let record = unsafe { &mut *owner.patches.get() };
    if !patches::apply(record, game_module(), |message| owner.log_text(message)) {
        return false;
    }
    owner.active.store(true, Ordering::SeqCst);
    true
}

pub(crate) fn is_active(handle: Handle) -> bool {
    installed(handle) && unsafe { Renderer::from_handle(handle) }.active()
}

pub(crate) fn quiesce(handle: Handle) {
    if handle.is_null() {
        return;
    }
    let owner = unsafe { Renderer::from_handle(handle) };
    owner.quiescing.store(true, Ordering::SeqCst);
    owner.deactivate();
    if let Some(disable) = native().and_then(|calls| calls.controller_disable) {
        unsafe { disable() };
    }
    unsafe { rsf_ac7_render_scopes_quiesce(owner.scopes()) };
}

/// Retirement. True when the renderer is gone. Every refusal leaves a state the next call can
/// continue from: each step forgets only what it retired.
pub(crate) fn stop(handle: Handle) -> bool {
    if handle.is_null() {
        return true;
    }
    let owner = unsafe { Renderer::from_handle(handle) };
    if owner.active() {
        return false;
    }
    // Admission. Inactive CPU observers stay installed until their render-thread-only pool refs
    // retire: disabling first would remove the only owner that can drain an asynchronous RHI
    // release. Nothing changes before every check passes.
    drain_retired(owner);
    let outstanding = owner.live_renderers.load(Ordering::SeqCst) != 0
        || owner.live_window_tasks.load(Ordering::SeqCst) != 0
        || owner.live_frame_tasks.load(Ordering::SeqCst) != 0
        || owner.pending_uniforms.load(Ordering::SeqCst) != 0
        || owner.pending_particle_bindings.load(Ordering::SeqCst) != 0
        || !owner.retired.load(Ordering::SeqCst).is_null();
    if guards::held(false) != 0
        || outstanding
        || !unsafe { crate::render_scope::scopes_idle(owner.scopes()) }
    {
        return false;
    }
    let input_hook = owner.input_hook.load(Ordering::SeqCst);
    if !input_hook.is_null() {
        if unsafe { UnhookWindowsHookEx(HHOOK(input_hook)) }.is_err() {
            return false;
        }
        owner.input_hook.store(null_mut(), Ordering::SeqCst);
    }
    let controller_removed = native()
        .and_then(|calls| calls.controller_remove)
        .is_some_and(|remove| unsafe { remove() });
    if !disable_created() || guards::held(true) != 0 || !controller_removed {
        return false;
    }
    // Temporal velocity, then scene precision; a refusal is logged there.
    // SAFETY: as in `start`.
    let record = unsafe { &mut *owner.patches.get() };
    if !patches::restore(record, |message| owner.log_text(message)) {
        return false;
    }
    if !restore_truesky(owner) || !scopes_destroy(owner) {
        return false;
    }
    drain_retired(owner);
    if !owner.retired.load(Ordering::SeqCst).is_null() || !remove_created() {
        return false;
    }
    INSTALLED.store(null_mut(), Ordering::SeqCst);
    drop(unsafe { Box::from_raw(handle.cast::<Renderer>()) });
    true
}

#[cfg(test)]
mod tests {
    use super::environment_flag;
    use std::ffi::OsString;

    #[test]
    fn environment_flag_reads_as_the_former_eight_byte_buffer() {
        let flag = |text: &str| environment_flag(Some(&OsString::from(text)));
        assert!(!environment_flag(None));
        assert!(!flag(""));
        assert!(flag("1") && flag("yes") && flag("1234567"));
        assert!(!flag("0") && !flag("01"));
        // Eight characters did not fit the former buffer.
        assert!(!flag("12345678"));
    }
}
