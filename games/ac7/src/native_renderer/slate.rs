// SPDX-License-Identifier: MIT
//! Slate window ownership: the game-thread producer's source frame carried to the rendering
//! task, the window scope that owns its swapchain Present, and the final scene texture bind.
use super::frame::{AFTER_SIMULATION, INPUT_SOURCE_FRAME, INSIDE_ENGINE_TICK};
use super::{
    Guard, Renderer, WindowSource, at, close, game_engine_object, hook, installed, key, method,
    native_texture, open_leased, original, read, scope,
};
use crate::platform_abi::rsf_ac7_render_scope;
use crate::scope_abi::rsf_ac7_scope_lease;
use rsf_game_sys::{
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_TEXTURE_BINDING,
    RSF_GAME_RENDER_WINDOW,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::{IUnknown, Interface};

thread_local! {
    static PRODUCER_SOURCE: Cell<WindowSource> = const { Cell::new(WindowSource::EMPTY) };
    static EXECUTION_SOURCE: Cell<WindowSource> = const { Cell::new(WindowSource::EMPTY) };
    static BINDING_SOURCE: Cell<WindowSource> = const { Cell::new(WindowSource::EMPTY) };
}

pub(super) unsafe extern "C" fn hooked_slate_private(renderer: *mut c_void, buffer: *mut c_void) {
    let _lifetime = Guard::outer();
    let saved = PRODUCER_SOURCE.replace(WindowSource::EMPTY);
    let object;
    let (mut client, mut viewport, mut window, mut control) = (0u64, 0u64, 0u64, 0u64);
    let mut refs = 0i32;
    if installed().is_some_and(Renderer::active)
        && INSIDE_ENGINE_TICK.get()
        && INPUT_SOURCE_FRAME.get() != 0
        && {
            object = game_engine_object();
            object != 0
        }
        && read(object, 0x720, &mut client)
        && read(client, 0xa0, &mut viewport)
        && viewport != 0
        && read(object, 0xdf8, &mut window)
        && window != 0
        && read(object, 0xe00, &mut control)
        && read(control, 8, &mut refs)
        && refs > 0
    {
        PRODUCER_SOURCE.set(WindowSource {
            source: INPUT_SOURCE_FRAME.get(),
            viewport,
            window,
            renderer: key(renderer),
            after_simulation: AFTER_SIMULATION.get(),
            ..WindowSource::EMPTY
        });
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { original(hook::SLATE_PRIVATE) };
    unsafe { forward(renderer, buffer) };
    PRODUCER_SOURCE.set(saved);
}

pub(super) unsafe extern "C" fn hooked_slate_allocate(
    result: *mut c_void,
    prerequisite: u64,
    priority: u32,
) -> *mut c_void {
    let _producer = Guard::entry();
    let allocate: unsafe extern "C" fn(*mut c_void, u64, u32) -> *mut c_void =
        unsafe { original(hook::SLATE_ALLOCATE) };
    let returned = unsafe { allocate(result, prerequisite, priority) };
    let producer = PRODUCER_SOURCE.get();
    let mut task = 0u64;
    if let Some(owner) = installed()
        && owner.active()
        && producer.source != 0
        && read(key(returned), 0, &mut task)
        && task != 0
    {
        let mut refused = true;
        {
            let mut tables = owner.tables();
            if let Some(slot) = tables
                .window_tasks
                .iter_mut()
                .find(|slot| slot.task == task)
            {
                // A missed task retirement must not associate a reused address with its old
                // source frame. Preserve the occupied entry but make its identity unusable.
                slot.source = 0;
            } else if let Some(slot) = tables.window_tasks.iter_mut().find(|slot| slot.task == 0) {
                *slot = producer;
                slot.task = task;
                owner.live_window_tasks.fetch_add(1, Ordering::SeqCst);
                refused = false;
            }
        }
        if refused {
            owner.log(
                c"AC7 window source refused: duplicate or full live task table; no binding evicted",
            );
        }
    }
    returned
}

pub(super) unsafe extern "C" fn hooked_slate_task(task: *mut c_void) {
    let _lifetime = Guard::outer();
    let saved = EXECUTION_SOURCE.replace(WindowSource::EMPTY);
    let mut source = WindowSource::EMPTY;
    if let Some(owner) = installed() {
        let _producer = Guard::entry();
        let mut tables = owner.tables();
        if let Some(slot) = tables
            .window_tasks
            .iter_mut()
            .find(|slot| slot.task == key(task))
        {
            source = *slot;
            *slot = WindowSource::EMPTY;
            owner.live_window_tasks.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let mut window = 0u64;
    if source.source != 0
        && read(source.task, 0x28, &mut window)
        && window == source.window
        && read(source.task, 0x10, &mut source.renderer)
        && read(source.task, 0x18, &mut source.info)
        && read(source.task, 0x20, &mut source.elements)
    {
        EXECUTION_SOURCE.set(source);
    }
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::SLATE_TASK) };
    unsafe { forward(task) };
    EXECUTION_SOURCE.set(saved);
}

/// The lease object of a window scope is its owned IDXGISwapChain reference.
unsafe extern "C" fn resolve_window(object: *mut c_void, scope: *mut rsf_ac7_render_scope) {
    unsafe { (*scope).swapchain = object };
}

/// The lease object of a texture binding scope is its owned ID3D11Resource reference.
unsafe extern "C" fn resolve_texture_binding(
    object: *mut c_void,
    scope: *mut rsf_ac7_render_scope,
) {
    unsafe { (*scope).sampled_texture = object };
}

unsafe extern "C" fn release_com(object: *mut c_void) {
    unsafe { crate::com_resource::release(object) };
}

static WINDOW_REFUSALS: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn hooked_slate_window(
    renderer: *mut c_void,
    list: *mut c_void,
    info: *mut c_void,
    elements: *mut c_void,
    vsync: u8,
    clear: u8,
) {
    let _lifetime = Guard::outer();
    let owner = installed();
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, u8, u8) =
        unsafe { original(hook::SLATE_WINDOW) };
    let mut source = EXECUTION_SOURCE.get();
    let producer = PRODUCER_SOURCE.get();
    let element_key = key(elements);
    if source.source == 0 && producer.source != 0 {
        let mut window = 0u64;
        read(element_key, 0x10, &mut window);
        if window == 0 {
            read(element_key, 0, &mut window);
        }
        if window == producer.window {
            source = producer;
            source.info = key(info);
            source.elements = element_key;
        }
    }
    let mut ticket = None;
    if let Some(owner) = owner
        && owner.active()
        && source.source != 0
        && source.window != 0
        && source.renderer == key(renderer)
        && source.info == key(info)
        && source.elements == element_key
    {
        let mut rhi = 0u64;
        if read(source.info, 0x70, &mut rhi) {
            // FRHIViewport's first method after its destructor is GetNativeSwapChain. The
            // matched DX11 viewport override returns IDXGISwapChain, not a texture resource.
            let getter = method(rhi, 8);
            let native = if getter == 0 {
                std::ptr::null_mut()
            } else {
                let call: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
                    unsafe { super::function(at(getter)) };
                unsafe { call(at(rhi)) }
            };
            let swapchain = unsafe { IUnknown::from_raw_borrowed(&native) }
                .and_then(|native| native.cast::<IDXGISwapChain>().ok());
            if let Some(swapchain) = swapchain {
                let mut window = scope();
                window.session_id = owner.options.session_id;
                window.role = RSF_GAME_RENDER_WINDOW;
                window.source_frame_id = source.source;
                window.viewport_key = source.viewport;
                window.window_key = source.window;
                window.rhi_viewport_key = rhi;
                window.pass_key = if source.task != 0 {
                    source.task
                } else {
                    element_key
                };
                window.flags = RSF_GAME_RENDER_PRIMARY;
                if source.after_simulation {
                    window.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
                }
                let lease = rsf_ac7_scope_lease {
                    object: swapchain.into_raw(),
                    resolve: Some(resolve_window),
                    release: Some(release_com),
                };
                ticket = open_leased(owner, list, &window, &lease);
                if ticket.is_none() {
                    unsafe { release_com(lease.object) };
                    if WINDOW_REFUSALS.fetch_add(1, Ordering::SeqCst) < 4 {
                        owner.log(
                            c"AC7 window scope refused: source Present ownership was not queued",
                        );
                    }
                }
            }
        }
    }
    let saved_binding = BINDING_SOURCE.replace(if ticket.is_some() {
        source
    } else {
        WindowSource::EMPTY
    });
    unsafe { forward(renderer, list, info, elements, vsync, clear) };
    BINDING_SOURCE.set(saved_binding);
    if let (Some(ticket), Some(owner)) = (ticket, owner)
        && !close(ticket, list)
    {
        owner.deactivate();
        owner.log(c"AC7 window source deactivated: native RHI scope could not close");
    }
}

static BINDING_REFUSALS: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn hooked_slate_texture(
    shader: *mut c_void,
    list: *mut c_void,
    rhi_texture: *mut c_void,
    sampler_ref: *mut c_void,
) {
    let _producer = Guard::entry();
    let owner = installed();
    let source = BINDING_SOURCE.get();
    let mut ticket = None;
    let (mut count, mut slot) = (0u16, 0u16);
    if let Some(owner) = owner
        && owner.active()
        && source.source != 0
        && source.window != 0
        && read(key(shader), 0xc2, &mut count)
        && count != 0
        && read(key(shader), 0xc0, &mut slot)
    {
        let texture = native_texture(key(rhi_texture));
        let slot_index = (source.source % owner.final_surface_hint.len() as u64) as usize;
        // Most Slate batches are fonts/icons. Only a surface produced by this exact scene source
        // needs a queued validation marker and resource lease. The lock-free hint rejects the
        // rest, so the identity lock is taken only for a bind of a published final surface.
        let mut candidate = !texture.is_null()
            && owner.final_surface_hint[slot_index].load(Ordering::Acquire) == key(texture);
        if candidate {
            let tables = owner.tables();
            let surface = &tables.final_surfaces[slot_index];
            candidate = surface.source == source.source
                && surface.viewport == source.viewport
                && !surface.ambiguous
                && surface.surface == key(texture);
        }
        if candidate {
            unsafe { crate::com_resource::retain(texture) };
            let mut binding = scope();
            binding.session_id = owner.options.session_id;
            binding.role = RSF_GAME_RENDER_TEXTURE_BINDING;
            binding.source_frame_id = source.source;
            binding.viewport_key = source.viewport;
            binding.window_key = source.window;
            binding.pass_key = key(shader);
            binding.texture_slot = u32::from(slot);
            binding.flags = RSF_GAME_RENDER_PRIMARY;
            if source.after_simulation {
                binding.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
            }
            let lease = rsf_ac7_scope_lease {
                object: texture,
                resolve: Some(resolve_texture_binding),
                release: Some(release_com),
            };
            ticket = open_leased(owner, list, &binding, &lease);
            if ticket.is_none() {
                unsafe { release_com(texture) };
                if BINDING_REFUSALS.fetch_add(1, Ordering::SeqCst) < 4 {
                    owner.log(c"AC7 texture-binding scope refused: final scene/window association was not queued");
                }
            }
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) =
        unsafe { original(hook::SLATE_TEXTURE) };
    unsafe { forward(shader, list, rhi_texture, sampler_ref) };
    if let (Some(ticket), Some(owner)) = (ticket, owner)
        && !close(ticket, list)
    {
        owner.deactivate();
        owner.log(c"AC7 texture binding deactivated: native RHI scope could not close");
    }
}
