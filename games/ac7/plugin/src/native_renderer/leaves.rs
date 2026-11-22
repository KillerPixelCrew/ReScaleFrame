// SPDX-License-Identifier: MIT
//! The leaves the renderer transaction (renderer.rs) calls,
//! the code compare and protected write leaves, and the MinHook records of every detour. The
//! transaction decides their order and refusals; each leaf touches only the renderer object, a
//! MinHook record, the module or, through truesky.rs, the TrueSky plugin, and none unwinds.
use super::sky::restore_truesky;
use super::{
    HOOKS, Renderer, SITES, bytes_match, drain_retired, game_module, hook, native,
    rsf_ac7_render_scopes_destroy, rsf_ac7_render_scopes_idle, rsf_ac7_render_scopes_quiesce,
};
use super::{evidence, frame, graph, particles, shaders, sky, slate, translucency, views, widgets};
use crate::sys::{rsf_ac7_code_leaves, rsf_ac7_code_patches};
use crate::sys::{
    rsf_ac7_native_renderer, rsf_ac7_native_renderer_options, rsf_ac7_renderer_log_fn,
};
use rsf_minhook_sys::{MH_CreateHook, MH_DisableHook, MH_EnableHook, MH_Initialize, MH_RemoveHook};
use std::ffi::c_void;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
};
use windows::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect,
};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::UI::WindowsAndMessaging::{HHOOK, UnhookWindowsHookEx};
use windows::core::PCWSTR;

const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;
const MH_ERROR_NOT_CREATED: i32 = 4;
const MH_ERROR_DISABLED: i32 = 6;

unsafe extern "C" fn matches(address: *const u8, expected: *const u8, count: usize) -> bool {
    bytes_match(address.cast(), expected.cast(), count)
}

/// Replaces `size` bytes at `site` that equal `expected` under code protection and flushes the
/// instruction cache. False leaves the code unchanged.
unsafe extern "C" fn write_render_code(
    site: *mut u8,
    expected: *const u8,
    replacement: *const u8,
    size: usize,
) -> bool {
    if size == 0 || !bytes_match(site.cast(), expected.cast(), size) {
        return false;
    }
    let mut protection = PAGE_PROTECTION_FLAGS(0);
    if unsafe { VirtualProtect(site.cast(), size, PAGE_EXECUTE_READWRITE, &mut protection) }
        .is_err()
    {
        return false;
    }
    unsafe { copy_nonoverlapping(replacement, site, size) };
    let _ = unsafe { FlushInstructionCache(GetCurrentProcess(), Some(site.cast()), size) };
    let mut ignored = PAGE_PROTECTION_FLAGS(0);
    let _ = unsafe { VirtualProtect(site.cast(), size, protection, &mut ignored) };
    true
}

/// The code leaves lent to the Rust site and patch owners (sites.rs, patches.rs, truesky.rs).
pub(crate) fn code_leaves(log: rsf_ac7_renderer_log_fn, user: *mut c_void) -> rsf_ac7_code_leaves {
    rsf_ac7_code_leaves {
        matches: Some(matches),
        write: Some(write_render_code),
        log,
        user,
    }
}

fn detour(address: *const c_void) -> *mut c_void {
    address.cast_mut()
}

/// Attaches each detour to its site. Keyed by hook, so the order of this list carries no meaning.
fn bind_detours() {
    use hook::*;
    let bindings: [(u32, *mut c_void); HOOKS] = [
        (
            PROCESS_TONEMAP,
            detour(graph::hooked_process::<PROCESS_TONEMAP> as *const c_void),
        ),
        (
            PROCESS_AA,
            detour(graph::hooked_process::<PROCESS_AA> as *const c_void),
        ),
        (
            PROCESS_MATERIAL,
            detour(graph::hooked_process::<PROCESS_MATERIAL> as *const c_void),
        ),
        (
            PROCESS_HUD,
            detour(graph::hooked_process::<PROCESS_HUD> as *const c_void),
        ),
        (
            PROCESS_COMPOSITE,
            detour(graph::hooked_process::<PROCESS_COMPOSITE> as *const c_void),
        ),
        (
            PROCESS_OUTPUT,
            detour(graph::hooked_process::<PROCESS_OUTPUT> as *const c_void),
        ),
        (
            DESCRIPTOR_TONEMAP,
            detour(graph::hooked_descriptor::<PROCESS_TONEMAP> as *const c_void),
        ),
        (
            DESCRIPTOR_AA,
            detour(graph::hooked_descriptor::<PROCESS_AA> as *const c_void),
        ),
        (
            DESCRIPTOR_MATERIAL,
            detour(graph::hooked_descriptor::<PROCESS_MATERIAL> as *const c_void),
        ),
        (
            DESCRIPTOR_HUD,
            detour(graph::hooked_descriptor::<PROCESS_HUD> as *const c_void),
        ),
        (
            DESCRIPTOR_COMPOSITE,
            detour(graph::hooked_descriptor::<PROCESS_COMPOSITE> as *const c_void),
        ),
        (
            DESCRIPTOR_OUTPUT,
            detour(graph::hooked_descriptor::<PROCESS_OUTPUT> as *const c_void),
        ),
        (
            POSTPROCESS,
            detour(graph::hooked_postprocess as *const c_void),
        ),
        (CONTEXT, detour(graph::hooked_context as *const c_void)),
        (
            VISIBILITY,
            detour(views::hooked_visibility as *const c_void),
        ),
        (CONSTRUCT, detour(views::hooked_construct as *const c_void)),
        (
            WIDGET_QUEUE,
            detour(widgets::hooked_widget_queue as *const c_void),
        ),
        (
            WIDGET_TARGETS,
            detour(widgets::hooked_widget_targets as *const c_void),
        ),
        (
            WIDGET_DRAW,
            detour(widgets::hooked_widget_draw as *const c_void),
        ),
        (
            ENGINE_TICK,
            detour(frame::hooked_engine_tick as *const c_void),
        ),
        (
            RENDERER_RETIRE,
            detour(frame::hooked_renderer_retire as *const c_void),
        ),
        (
            POLL_INPUT,
            detour(frame::hooked_poll_input as *const c_void),
        ),
        (
            GAME_INSTANCE_INIT,
            detour(widgets::hooked_game_instance_init as *const c_void),
        ),
        (
            TARGET_INIT,
            detour(widgets::hooked_target_init as *const c_void),
        ),
        (
            SIMULATION_A,
            detour(frame::hooked_simulation::<SIMULATION_A> as *const c_void),
        ),
        (
            SIMULATION_B,
            detour(frame::hooked_simulation::<SIMULATION_B> as *const c_void),
        ),
        (REDRAW, detour(frame::hooked_redraw as *const c_void)),
        (
            VIEWPORT_DRAW,
            detour(frame::hooked_viewport_draw as *const c_void),
        ),
        (
            SLATE_PRIVATE,
            detour(slate::hooked_slate_private as *const c_void),
        ),
        (
            SLATE_ALLOCATE,
            detour(slate::hooked_slate_allocate as *const c_void),
        ),
        (
            SLATE_TASK,
            detour(slate::hooked_slate_task as *const c_void),
        ),
        (
            SLATE_WINDOW,
            detour(slate::hooked_slate_window as *const c_void),
        ),
        (
            SLATE_TEXTURE,
            detour(slate::hooked_slate_texture as *const c_void),
        ),
        (
            TRANSLUCENCY_SIZE,
            detour(translucency::hooked_translucency_size as *const c_void),
        ),
        (
            UNMODIFIED_BEGIN,
            detour(translucency::hooked_unmodified_begin as *const c_void),
        ),
        (
            UNMODIFIED_RESOLVE,
            detour(translucency::hooked_unmodified_resolve as *const c_void),
        ),
        (
            TRANSLUCENCY_RENDER,
            detour(translucency::hooked_translucency_render as *const c_void),
        ),
        (
            PIXEL_VIEW_UNIFORM,
            detour(evidence::hooked_pixel_view_uniform as *const c_void),
        ),
        (
            RHI_PIXEL_UNIFORM,
            detour(evidence::hooked_rhi_pixel_uniform as *const c_void),
        ),
        (
            RHI_PIXEL_TABLES,
            detour(evidence::hooked_rhi_pixel_tables as *const c_void),
        ),
        (
            PIXEL_ENQUEUE_A,
            detour(evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_A> as *const c_void),
        ),
        (
            PIXEL_ENQUEUE_B,
            detour(evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_B> as *const c_void),
        ),
        (
            PIXEL_ENQUEUE_C,
            detour(evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_C> as *const c_void),
        ),
        (
            PIXEL_ENQUEUE_D,
            detour(evidence::hooked_pixel_enqueue::<PIXEL_ENQUEUE_D> as *const c_void),
        ),
        (
            SKY_PROJECTION,
            detour(sky::hooked_sky_projection as *const c_void),
        ),
        (SKY_RENDER, detour(sky::hooked_sky_render as *const c_void)),
        (
            TEMPORAL_SAMPLE_INDEX,
            detour(views::hooked_temporal_sample_index as *const c_void),
        ),
        (
            SCENE_COLOUR_FORMAT,
            detour(translucency::hooked_scene_colour_format as *const c_void),
        ),
        (
            RENDER_FAMILY,
            detour(frame::hooked_render_family as *const c_void),
        ),
        (
            FRAME_TASK_CONSTRUCT,
            detour(frame::hooked_frame_task_construct as *const c_void),
        ),
        (
            FRAME_TASK_EXECUTE,
            detour(frame::hooked_frame_task_execute as *const c_void),
        ),
        (
            RHI_FRAME_BEGIN,
            detour(frame::hooked_rhi_frame_begin as *const c_void),
        ),
        (
            RHI_FRAME_END,
            detour(frame::hooked_rhi_frame_end as *const c_void),
        ),
        (
            FRAME_SYNC,
            detour(frame::hooked_frame_sync as *const c_void),
        ),
        (
            PUMP_MESSAGES,
            detour(frame::hooked_pump_messages as *const c_void),
        ),
        (
            ENGINE_PACING,
            detour(frame::hooked_engine_pacing as *const c_void),
        ),
        (
            CREATE_PIXEL_SHADER,
            detour(shaders::hooked_create_pixel_shader as *const c_void),
        ),
        (
            BASE_PASS,
            detour(translucency::hooked_base_pass as *const c_void),
        ),
        (
            TRANSLUCENCY_MESH,
            detour(translucency::hooked_translucency_mesh as *const c_void),
        ),
        (
            MESH_PARTICLE_PARAMETERS,
            detour(translucency::hooked_mesh_particle_parameters as *const c_void),
        ),
        (
            BEAM_NO_NOISE,
            detour(particles::hooked_beam_fill::<BEAM_NO_NOISE> as *const c_void),
        ),
        (
            BEAM_NOISE,
            detour(particles::hooked_beam_fill::<BEAM_NOISE> as *const c_void),
        ),
        (
            BEAM_INTERPOLATED_NOISE,
            detour(particles::hooked_beam_fill::<BEAM_INTERPOLATED_NOISE> as *const c_void),
        ),
        (
            RIBBON_FILL,
            detour(particles::hooked_beam_fill::<RIBBON_FILL> as *const c_void),
        ),
        (
            ANIM_TRAIL_FILL,
            detour(particles::hooked_beam_fill::<ANIM_TRAIL_FILL> as *const c_void),
        ),
    ];
    for (hook, detour) in bindings {
        SITES[hook as usize].detour.store(detour, Ordering::Relaxed);
    }
}

fn site(hook: u32) -> Option<&'static super::Site> {
    SITES.get(hook as usize)
}

fn bound_target(hook: u32) -> Option<*mut c_void> {
    site(hook)
        .map(|site| site.target.load(Ordering::Relaxed))
        .filter(|target| !target.is_null())
}

/// # Safety
/// `handle` is a live renderer from `create`.
unsafe fn renderer_ref<'a>(handle: *mut rsf_ac7_native_renderer) -> &'a Renderer {
    unsafe { Renderer::from_handle(handle) }
}

pub(crate) unsafe extern "C" fn installed() -> *mut rsf_ac7_native_renderer {
    super::INSTALLED.load(Ordering::SeqCst).cast()
}

pub(crate) unsafe extern "C" fn install(renderer: *mut rsf_ac7_native_renderer) {
    super::INSTALLED.store(renderer.cast(), Ordering::SeqCst);
}

pub(crate) unsafe extern "C" fn create(
    options: *const rsf_ac7_native_renderer_options,
) -> *mut rsf_ac7_native_renderer {
    match unsafe { options.as_ref() }.and_then(Renderer::create) {
        Some(renderer) => Box::into_raw(renderer).cast(),
        None => null_mut(),
    }
}

pub(crate) unsafe extern "C" fn release(renderer: *mut rsf_ac7_native_renderer) {
    if !renderer.is_null() {
        drop(unsafe { Box::from_raw(renderer.cast::<Renderer>()) });
    }
}

pub(crate) unsafe extern "C" fn options(
    renderer: *mut rsf_ac7_native_renderer,
) -> *const rsf_ac7_native_renderer_options {
    &unsafe { renderer_ref(renderer) }.options
}

pub(crate) unsafe extern "C" fn patches(
    renderer: *mut rsf_ac7_native_renderer,
) -> *mut rsf_ac7_code_patches {
    unsafe { renderer_ref(renderer) }.patches.get()
}

pub(crate) unsafe extern "C" fn active(renderer: *mut rsf_ac7_native_renderer) -> bool {
    unsafe { renderer_ref(renderer) }.active()
}

pub(crate) unsafe extern "C" fn set_active(renderer: *mut rsf_ac7_native_renderer, active: bool) {
    unsafe { renderer_ref(renderer) }
        .active
        .store(active, Ordering::SeqCst);
}

pub(crate) unsafe extern "C" fn quiescing(renderer: *mut rsf_ac7_native_renderer) -> bool {
    unsafe { renderer_ref(renderer) }.quiescing()
}

pub(crate) unsafe extern "C" fn set_quiescing(renderer: *mut rsf_ac7_native_renderer) {
    unsafe { renderer_ref(renderer) }
        .quiescing
        .store(true, Ordering::SeqCst);
}

pub(crate) unsafe extern "C" fn scopes_create(
    renderer: *mut rsf_ac7_native_renderer,
    capacity: u32,
) -> bool {
    let owner = unsafe { renderer_ref(renderer) };
    let options = &owner.options;
    let Some(create) = native().and_then(|calls| calls.scopes_create) else {
        return false;
    };
    let mut scopes = owner.scopes.load(Ordering::Relaxed);
    let created = unsafe {
        create(
            options.session_id,
            capacity,
            options.on_state,
            options.on_stage,
            options.user,
            &mut scopes,
        )
    } != 0;
    owner.scopes.store(scopes, Ordering::SeqCst);
    created
}

pub(crate) unsafe extern "C" fn scopes_quiesce(renderer: *mut rsf_ac7_native_renderer) {
    unsafe { rsf_ac7_render_scopes_quiesce(renderer_ref(renderer).scopes()) };
}

pub(crate) unsafe extern "C" fn scopes_idle(renderer: *mut rsf_ac7_native_renderer) -> bool {
    unsafe { rsf_ac7_render_scopes_idle(renderer_ref(renderer).scopes()) != 0 }
}

pub(crate) unsafe extern "C" fn scopes_destroy(renderer: *mut rsf_ac7_native_renderer) -> bool {
    let owner = unsafe { renderer_ref(renderer) };
    if unsafe { rsf_ac7_render_scopes_destroy(owner.scopes()) } == 0 {
        return false;
    }
    owner.scopes.store(null_mut(), Ordering::SeqCst);
    true
}

pub(crate) unsafe extern "C" fn hooks_initialize() -> bool {
    let init = unsafe { MH_Initialize() };
    if init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED {
        return false;
    }
    bind_detours();
    true
}

pub(crate) unsafe extern "C" fn hook_bind(hook: u32, target: *mut u8) {
    if let Some(site) = site(hook) {
        site.target.store(target.cast(), Ordering::Relaxed);
    }
}

pub(crate) unsafe extern "C" fn hook_create(hook: u32) -> bool {
    let Some(target) = bound_target(hook) else {
        return false;
    };
    let site = &SITES[hook as usize];
    let created = unsafe {
        MH_CreateHook(
            target,
            site.detour.load(Ordering::Relaxed),
            site.original.as_ptr(),
        )
    };
    created == MH_OK
}

pub(crate) unsafe extern "C" fn hook_enable(hook: u32) -> bool {
    bound_target(hook).is_some_and(|target| unsafe { MH_EnableHook(target) } == MH_OK)
}

/// MH_CreateHook writes the trampoline only on success; a removal clears it again.
pub(crate) unsafe extern "C" fn hook_created(hook: u32) -> bool {
    bound_target(hook).is_some()
        && !SITES[hook as usize]
            .original
            .load(Ordering::Relaxed)
            .is_null()
}

/// A null target is a hook that was never installed; MH_DisableHook(NULL) would disable them all.
pub(crate) unsafe extern "C" fn hook_disable(hook: u32) -> bool {
    let Some(target) = bound_target(hook) else {
        return false;
    };
    let disabled = unsafe { MH_DisableHook(target) };
    disabled == MH_OK || disabled == MH_ERROR_DISABLED || disabled == MH_ERROR_NOT_CREATED
}

pub(crate) unsafe extern "C" fn hook_remove(hook: u32) -> bool {
    let Some(target) = bound_target(hook) else {
        return false;
    };
    let removed = unsafe { MH_RemoveHook(target) };
    if removed != MH_OK && removed != MH_ERROR_NOT_CREATED {
        return false;
    }
    SITES[hook as usize]
        .original
        .store(null_mut(), Ordering::Relaxed);
    true
}

pub(crate) unsafe extern "C" fn pin() {
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

pub(crate) unsafe extern "C" fn drain(renderer: *mut rsf_ac7_native_renderer) {
    drain_retired(unsafe { renderer_ref(renderer) });
}

pub(crate) unsafe extern "C" fn outstanding(renderer: *mut rsf_ac7_native_renderer) -> bool {
    let owner = unsafe { renderer_ref(renderer) };
    owner.live_renderers.load(Ordering::SeqCst) != 0
        || owner.live_window_tasks.load(Ordering::SeqCst) != 0
        || owner.live_frame_tasks.load(Ordering::SeqCst) != 0
        || owner.pending_uniforms.load(Ordering::SeqCst) != 0
        || owner.pending_particle_bindings.load(Ordering::SeqCst) != 0
        || !owner.retired.load(Ordering::SeqCst).is_null()
}

pub(crate) unsafe extern "C" fn retired(renderer: *mut rsf_ac7_native_renderer) -> bool {
    !unsafe { renderer_ref(renderer) }
        .retired
        .load(Ordering::SeqCst)
        .is_null()
}

pub(crate) unsafe extern "C" fn input_unhook(renderer: *mut rsf_ac7_native_renderer) -> bool {
    let owner = unsafe { renderer_ref(renderer) };
    let hook = owner.input_hook.load(Ordering::SeqCst);
    if hook.is_null() {
        return true;
    }
    if unsafe { UnhookWindowsHookEx(HHOOK(hook)) }.is_err() {
        return false;
    }
    owner.input_hook.store(null_mut(), Ordering::SeqCst);
    true
}

pub(crate) unsafe extern "C" fn controller_disable() {
    if let Some(disable) = native().and_then(|calls| calls.controller_disable) {
        unsafe { disable() };
    }
}

pub(crate) unsafe extern "C" fn controller_remove() -> bool {
    native()
        .and_then(|calls| calls.controller_remove)
        .is_some_and(|remove| unsafe { remove() })
}

pub(crate) unsafe extern "C" fn truesky_restore(renderer: *mut rsf_ac7_native_renderer) -> bool {
    restore_truesky(unsafe { renderer_ref(renderer) })
}

/// The loaded image base the renderer's code sites are relative to.
pub(crate) fn base() -> *mut u8 {
    game_module()
}
