// SPDX-License-Identifier: MIT
//! CPU frame identity: the game-thread tick, simulation, input and submission markers, the
//! renderer identity bound into render packets, and the RHI frame and family scopes.
use super::graph::retire_renderer_uniforms;
use super::{
    Guard, Renderer, RendererIdentity, close, copy, drain_retired, game_engine_object, game_module,
    hook, installed, key, native, open, original, read, scope,
};
use crate::sys::rsf_ac7_render_scope;
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END,
    RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_SAMPLE,
    RSF_GAME_CPU_INPUT_UNFRAMED, RSF_GAME_CPU_SIMULATION_BEGIN, RSF_GAME_CPU_SIMULATION_END,
    RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_INPUT_CONTROLLER, RSF_GAME_RENDER_AFTER_SIMULATION,
    RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET,
    RSF_GAME_RENDER_SUBMISSION, RSF_SCREEN_BRIEFING, RSF_SCREEN_FLIGHT, RSF_SCREEN_HANGAR,
    RSF_SCREEN_REPLAY, RSF_SCREEN_UNKNOWN, rsf_game_cpu_event,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, MSG, PM_REMOVE, SetWindowsHookExW, WH_GETMESSAGE,
};

thread_local! {
    pub(super) static INSIDE_ENGINE_TICK: Cell<bool> = const { Cell::new(false) };
    static INSIDE_SIMULATION: Cell<bool> = const { Cell::new(false) };
    pub(super) static AFTER_SIMULATION: Cell<bool> = const { Cell::new(false) };
    static SIMULATION_CLOSED: Cell<bool> = const { Cell::new(false) };
    static SIMULATION_STARTED: Cell<bool> = const { Cell::new(false) };
    static INPUT_SAMPLED: Cell<bool> = const { Cell::new(false) };
    pub(super) static INPUT_SOURCE_FRAME: Cell<u64> = const { Cell::new(0) };
    static RESERVED_SOURCE_FRAME: Cell<u64> = const { Cell::new(0) };
    static SOURCE_RENDER_EXPECTED: Cell<u32> = const { Cell::new(0) };
    static SUBMITTING_VIEWPORT: Cell<u64> = const { Cell::new(0) };
    pub(super) static SHARED_REPAINT_TARGETS: Cell<[u64; 2]> = const { Cell::new([0; 2]) };
    static CONTROLLER_POLL_SCOPE: Cell<bool> = const { Cell::new(false) };
    static FRAME_TASK_SOURCE: Cell<u64> = const { Cell::new(0) };
    static FULL_FRAME_TICKET: Cell<*mut c_void> = const { Cell::new(null_mut()) };
}

fn object_has_class(object: u64, cache_rva: u32) -> bool {
    let (mut expected, mut actual, mut bases, mut entry) = (0u64, 0u64, 0u64, 0u64);
    let (mut depth, mut actual_depth) = (0i32, 0i32);
    object != 0
        && copy(
            (&mut expected as *mut u64).cast(),
            game_module().wrapping_add(cache_rva as usize).cast(),
            8,
        )
        && expected != 0
        && read(object, 0x10, &mut actual)
        && read(expected, 0x90, &mut depth)
        && read(actual, 0x90, &mut actual_depth)
        && depth >= 0
        && depth <= actual_depth
        && actual_depth <= 4096
        && read(actual, 0x88, &mut bases)
        && read(bases, depth as usize * 8, &mut entry)
        && entry == expected.wrapping_add(0x88)
}

/// Copy classification with the renderer. A later world at Present cannot establish which
/// screen produced this frame. Unsupported ownership, pause and camera transitions refuse FG.
/// `why` is zero or class bits for an accepted screen, and a reason code in its low byte for a
/// refused one: 1 unreadable player chain, 2 no view target, 3 a pending view target, 4 level
/// state unreadable, 5 paused, 6 no known scene owner. Bits from 0x100 name what was found:
/// 0x100 target is the hangar mesh manager, 0x200 target is the hangar pawn, 0x400 target is the
/// hangar plane selector, 0x800 pawn is the hangar pawn, 0x1000 target is the pawn, 0x2000 pawn
/// is the player plane, 0x4000 the world's game mode is the hangar's, 0x8000 target is the asset
/// viewer manager, 0x10000 target is an engine camera actor. `view_target` returns the actor
/// that owns the camera, so a change of owner can reset history.
pub(super) fn render_screen(reset: &mut u32, why: &mut u32, view_target: &mut u64) -> u32 {
    let (mut client, mut world, mut instance, mut players) = (0u64, 0u64, 0u64, 0u64);
    let (mut player, mut controller, mut pawn, mut camera) = (0u64, 0u64, 0u64, 0u64);
    let (mut target, mut pending, mut level, mut settings) = (0u64, 0u64, 0u64, 0u64);
    let (mut pauser, mut hud, mut ui, mut layer) = (0u64, 0u64, 0u64, 0u64);
    let (mut focused, mut mode) = (0u64, 0u64);
    let mut count = 0i32;
    let (mut camera_input, mut vr, mut cut) = (0u8, 0u8, 0u8);
    *view_target = 0;
    let engine_object = game_engine_object();
    *why = 1;
    if !read(engine_object, 0x720, &mut client)
        || !read(client, 0x80, &mut world)
        || !read(client, 0x88, &mut instance)
        || !read(instance, 0x38, &mut players)
        || !read(instance, 0x40, &mut count)
        || count != 1
        || !read(players, 0, &mut player)
        || !read(player, 0x30, &mut controller)
        || !read(controller, 0x370, &mut pawn)
        || !read(controller, 0x400, &mut camera)
        || !read(camera, 0xfb0, &mut target)
    {
        return RSF_SCREEN_UNKNOWN;
    }
    *why = 2;
    if target == 0 {
        return RSF_SCREEN_UNKNOWN;
    }
    *view_target = target;
    *why = 3;
    if !read(camera, 0x15a0, &mut pending) || pending != 0 {
        return RSF_SCREEN_UNKNOWN;
    }
    *why = 4;
    if !read(camera, 0x1c99, &mut cut)
        || !read(world, 0x30, &mut level)
        || !read(level, 0x300, &mut settings)
        || !read(settings, 0x4e8, &mut pauser)
    {
        return RSF_SCREEN_UNKNOWN;
    }
    *why = 5;
    if pauser != 0 {
        return RSF_SCREEN_UNKNOWN;
    }
    if cut & 4 != 0 {
        *reset = 1;
    }
    // UWorld::AuthorityGameMode at +0xf0 (SDK reflection dump). A wrong offset fails the
    // class check and leaves this bit clear; it cannot accept a screen by itself.
    let hangar_mode = read(world, 0xf0, &mut mode) && object_has_class(mode, 0x3a8_2008);
    let bit = |set: bool, value: u32| if set { value } else { 0 };
    let found = bit(object_has_class(target, 0x3a8_2130), 0x100)
        | bit(object_has_class(target, 0x3a8_2140), 0x200)
        | bit(object_has_class(target, 0x3a8_2158), 0x400)
        | bit(object_has_class(pawn, 0x3a8_2140), 0x800)
        | bit(target == pawn, 0x1000)
        | bit(object_has_class(pawn, 0x3a8_4660), 0x2000)
        | bit(hangar_mode, 0x4000)
        | bit(object_has_class(target, 0x3a7_4730), 0x8000)
        | bit(object_has_class(target, 0x3cc_0150), 0x10000);
    *why = found;
    // The briefing owns a 3D scene. Its focused widget identifies this particular menu;
    // generic menu/video/loading frames remain unknown even when a background view exists.
    if read(controller, 0x3f8, &mut hud)
        && read(hud, 0x288, &mut ui)
        && read(ui, 0xf00, &mut layer)
        && read(layer, 0x820, &mut focused)
        && object_has_class(focused, 0x3a7_6c90)
    {
        return RSF_SCREEN_BRIEFING;
    }
    // The hangar is a level with its own game mode. Its aircraft viewer hands the camera to an
    // actor that is none of the hangar classes, so the level decides, not the camera owner.
    if found & 0x4f00 != 0 {
        return RSF_SCREEN_HANGAR;
    }
    // A rendered mission cinematic hands the view to CameraActor while the player plane
    // remains possessed. It is a real scene, not a video; pending targets/pause still refuse
    // above, and camera-owner changes and the native cut bit still reset temporal history.
    if found & 0x12000 == 0x12000 {
        return RSF_SCREEN_REPLAY;
    }
    *why = found | 6;
    if target != pawn
        || !object_has_class(pawn, 0x3a8_4660)
        || !read(pawn, 0xe12, &mut camera_input)
        || camera_input == 0
        || !read(pawn, 0xb52, &mut vr)
        || vr != 0
    {
        return RSF_SCREEN_UNKNOWN;
    }
    *why = found;
    RSF_SCREEN_FLIGHT
}

pub(super) fn cpu_event(
    owner: Option<&Renderer>,
    source: u64,
    stage: u32,
    kind: u32,
    message: u32,
) {
    let Some(owner) = owner else { return };
    let Some(callback) = owner.options.cpu_event else {
        return;
    };
    if source == 0 && stage != RSF_GAME_CPU_INPUT_EVENT {
        return;
    }
    let (mut now, mut frequency) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut now);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }
    let flags = RSF_GAME_CPU_INPUT_KEYED
        | if message != 0 {
            RSF_GAME_CPU_WINDOW_MESSAGE
        } else {
            0
        }
        | if source == 0 && kind != 0 {
            RSF_GAME_CPU_INPUT_UNFRAMED
        } else {
            0
        };
    let event = rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage,
        session_id: owner.options.session_id,
        source_frame_id: source,
        timestamp_qpc: now as u64,
        qpc_frequency: frequency as u64,
        render_expected: SOURCE_RENDER_EXPECTED.get(),
        input_kind: kind,
        message_id: message,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags,
    };
    unsafe { callback(owner.options.user, &event) };
}

unsafe extern "system" fn input_message(code: i32, removed: WPARAM, argument: LPARAM) -> LRESULT {
    let _lifetime = Guard::outer();
    let owner = installed();
    if code >= 0 && removed.0 == PM_REMOVE.0 as usize && owner.is_some_and(Renderer::active) {
        let message = unsafe { &*(argument.0 as *const MSG) };
        let kind = native()
            .and_then(|calls| calls.input_kind)
            .map_or(0, |kind| unsafe { kind(message.message, message.lParam.0) });
        let source = if INSIDE_ENGINE_TICK.get() {
            INPUT_SOURCE_FRAME.get()
        } else {
            0
        };
        cpu_event(
            owner,
            source,
            RSF_GAME_CPU_INPUT_EVENT,
            kind,
            message.message,
        );
    }
    unsafe { CallNextHookEx(None, code, removed, argument) }
}

pub(super) fn source_primary_renderer(owner: &Renderer, renderer: *mut c_void) -> bool {
    owner.config().is_some_and(|config| {
        crate::scope_facts::primary_renderer(
            key(renderer),
            config.output_width,
            config.output_height,
        )
    })
}

pub(super) fn bind_renderer(owner: &Renderer, object: *mut c_void) {
    if INPUT_SOURCE_FRAME.get() == 0 || !INSIDE_ENGINE_TICK.get() {
        return;
    }
    let primary = SUBMITTING_VIEWPORT.get() != 0
        && AFTER_SIMULATION.get()
        && source_primary_renderer(owner, object);
    let renderer = key(object);
    let mut refused = false;
    let mut screen_text = None;
    {
        let mut tables = owner.tables();
        if tables
            .identities
            .iter()
            .any(|slot| slot.renderer == renderer)
        {
            return;
        }
        match tables
            .identities
            .iter()
            .position(|entry| entry.renderer == 0)
        {
            None => refused = true,
            Some(empty) => {
                let (mut reset, mut why, mut view_target) = (0u32, 0u32, 0u64);
                let screen = if primary {
                    render_screen(&mut reset, &mut why, &mut view_target)
                } else {
                    RSF_SCREEN_UNKNOWN
                };
                if primary {
                    // A new camera owner is a hard cut even when the screen stays the same.
                    if tables.previous_primary_screen != screen
                        || tables.previous_view_target != view_target
                    {
                        reset = 1;
                    }
                    tables.previous_view_target = view_target;
                    // Bounded: says which rule accepted or refused the frame's screen, on change.
                    if (tables.previous_primary_screen != screen
                        || tables.previous_screen_why != why)
                        && tables.screen_lines < 96
                    {
                        tables.screen_lines += 1;
                        screen_text = Some(format!(
                            "AC7 screen {} -> {}, source {}, detail 0x{:x}",
                            tables.previous_primary_screen,
                            screen,
                            INPUT_SOURCE_FRAME.get(),
                            why
                        ));
                    }
                    tables.previous_primary_screen = screen;
                    tables.previous_screen_why = why;
                }
                let submission = tables.next_submission;
                tables.next_submission = submission.wrapping_add(1);
                tables.identities[empty] = RendererIdentity {
                    renderer,
                    source: INPUT_SOURCE_FRAME.get(),
                    submission,
                    viewport: SUBMITTING_VIEWPORT.get(),
                    after_simulation: AFTER_SIMULATION.get(),
                    screen,
                    reset,
                };
                if primary {
                    SOURCE_RENDER_EXPECTED.set(SOURCE_RENDER_EXPECTED.get().wrapping_add(1));
                }
                owner.live_renderers.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    if refused {
        owner.log(
            c"AC7 renderer identity refused: live renderer capacity reached; no association evicted",
        );
    }
    if let Some(text) = screen_text {
        owner.log_bounded(&text, 120);
    }
}

pub(super) fn renderer_identity(owner: &Renderer, pass: &mut rsf_ac7_render_scope) {
    if pass.family_key < 0x10 {
        return;
    }
    let tables = owner.tables();
    if let Some(slot) = tables
        .identities
        .iter()
        .find(|slot| slot.renderer == pass.family_key - 0x10)
    {
        pass.source_frame_id = slot.source;
        pass.submission_id = slot.submission;
        pass.viewport_key = slot.viewport;
        pass.screen = slot.screen;
        if slot.reset != 0 {
            pass.flags |= RSF_GAME_RENDER_RESET;
        }
        if slot.after_simulation {
            pass.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
        }
    }
}

pub(super) unsafe extern "C" fn hooked_engine_tick(engine_loop: *mut c_void) {
    let _entry = Guard::outer();
    let old_tick = INSIDE_ENGINE_TICK.get();
    let (old_source, old_reserved) = (INPUT_SOURCE_FRAME.get(), RESERVED_SOURCE_FRAME.get());
    let old_viewport = SUBMITTING_VIEWPORT.replace(0);
    let old_render_expected = SOURCE_RENDER_EXPECTED.replace(0);
    let owner = installed();
    let reserved = match owner {
        Some(owner) if owner.active() => owner.next_source.fetch_add(1, Ordering::SeqCst),
        _ => 0,
    };
    RESERVED_SOURCE_FRAME.set(reserved);
    let old_repaint = SHARED_REPAINT_TARGETS.get();
    let (old_simulation, old_after) = (INSIDE_SIMULATION.get(), AFTER_SIMULATION.get());
    let old_closed = SIMULATION_CLOSED.replace(false);
    let old_started = SIMULATION_STARTED.replace(false);
    let old_sampled = INPUT_SAMPLED.replace(false);
    INSIDE_SIMULATION.set(false);
    AFTER_SIMULATION.set(false);
    INSIDE_ENGINE_TICK.set(true);
    INPUT_SOURCE_FRAME.set(reserved);
    SHARED_REPAINT_TARGETS.set([0; 2]);
    // Not once quiesced: stop removes the hook from another thread and must not race a new one.
    if let Some(owner) = owner
        && !owner.quiescing()
        && owner.input_hook.load(Ordering::SeqCst).is_null()
    {
        let installed = unsafe {
            SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(input_message),
                None,
                GetCurrentThreadId(),
            )
        }
        .map_or(null_mut(), |hook| hook.0);
        owner.input_hook.store(installed, Ordering::SeqCst);
        owner.log(if installed.is_null() {
            c"AC7 input markers refused: game-thread message hook failed"
        } else {
            c"AC7 input markers: game-thread message dequeue and controller poll installed"
        });
    }
    cpu_event(owner, reserved, RSF_GAME_CPU_FRAME_BEGIN, 0, 0);
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::ENGINE_TICK) };
    unsafe { forward(engine_loop) };
    cpu_event(owner, reserved, RSF_GAME_CPU_FRAME_END, 0, 0);
    RESERVED_SOURCE_FRAME.set(old_reserved);
    SUBMITTING_VIEWPORT.set(old_viewport);
    INSIDE_ENGINE_TICK.set(old_tick);
    INPUT_SOURCE_FRAME.set(old_source);
    SHARED_REPAINT_TARGETS.set(old_repaint);
    INSIDE_SIMULATION.set(old_simulation);
    AFTER_SIMULATION.set(old_after);
    SIMULATION_CLOSED.set(old_closed);
    SIMULATION_STARTED.set(old_started);
    INPUT_SAMPLED.set(old_sampled);
    SOURCE_RENDER_EXPECTED.set(old_render_expected);
}

fn current_game_engine(object: *mut c_void) -> bool {
    !object.is_null() && game_engine_object() == key(object)
}

pub(super) unsafe extern "C" fn hooked_simulation<const INDEX: u32>(
    object: *mut c_void,
    delta: f32,
    idle: u8,
) {
    let _entry = Guard::outer();
    let owner = installed();
    let owns = owner.is_some_and(Renderer::active)
        && INSIDE_ENGINE_TICK.get()
        && INPUT_SOURCE_FRAME.get() != 0
        && !INSIDE_SIMULATION.get()
        && current_game_engine(object);
    if owns {
        INSIDE_SIMULATION.set(true);
        AFTER_SIMULATION.set(false);
        SIMULATION_STARTED.set(true);
        cpu_event(
            owner,
            INPUT_SOURCE_FRAME.get(),
            RSF_GAME_CPU_SIMULATION_BEGIN,
            0,
            0,
        );
    }
    let forward: unsafe extern "C" fn(*mut c_void, f32, u8) = unsafe { original(INDEX) };
    unsafe { forward(object, delta, idle) };
    if owns {
        INSIDE_SIMULATION.set(false);
    }
}

pub(super) unsafe extern "C" fn hooked_redraw(object: *mut c_void, present: u8) {
    let _entry = Guard::outer();
    // UGameEngine::Tick enters redraw after world/viewport updates. Bind this CPU state into
    // renderer packets instead of inspecting it later from the render or RHI thread.
    if INSIDE_ENGINE_TICK.get()
        && INSIDE_SIMULATION.get()
        && INPUT_SOURCE_FRAME.get() != 0
        && !AFTER_SIMULATION.get()
        && current_game_engine(object)
    {
        AFTER_SIMULATION.set(true);
    }
    let forward: unsafe extern "C" fn(*mut c_void, u8) = unsafe { original(hook::REDRAW) };
    unsafe { forward(object, present) };
}

pub(super) unsafe extern "C" fn hooked_viewport_draw(viewport: *mut c_void, should_present: u8) {
    let _lifetime = Guard::outer();
    let saved = SUBMITTING_VIEWPORT.replace(0);
    let (mut client, mut main_viewport) = (0u64, 0u64);
    if installed().is_some_and(Renderer::active)
        && INSIDE_ENGINE_TICK.get()
        && INPUT_SOURCE_FRAME.get() != 0
        && read(game_engine_object(), 0x720, &mut client)
        && read(client, 0xa0, &mut main_viewport)
        && main_viewport == key(viewport)
    {
        SUBMITTING_VIEWPORT.set(main_viewport);
    }
    let forward: unsafe extern "C" fn(*mut c_void, u8) = unsafe { original(hook::VIEWPORT_DRAW) };
    unsafe { forward(viewport, should_present) };
    SUBMITTING_VIEWPORT.set(saved);
}

unsafe extern "C" fn controller_received() {
    let owner = installed();
    if CONTROLLER_POLL_SCOPE.get()
        && owner.is_some_and(Renderer::active)
        && INPUT_SOURCE_FRAME.get() != 0
    {
        cpu_event(
            owner,
            INPUT_SOURCE_FRAME.get(),
            RSF_GAME_CPU_INPUT_EVENT,
            RSF_GAME_INPUT_CONTROLLER,
            0,
        );
    }
}

pub(super) unsafe extern "C" fn hooked_poll_input(slate: *mut c_void) {
    let _entry = Guard::entry();
    let owner = installed();
    let active = owner.is_some_and(Renderer::active);
    if active && INSIDE_ENGINE_TICK.get() && INPUT_SOURCE_FRAME.get() != 0 && !INPUT_SAMPLED.get() {
        INPUT_SAMPLED.set(true);
        cpu_event(
            owner,
            INPUT_SOURCE_FRAME.get(),
            RSF_GAME_CPU_INPUT_SAMPLE,
            0,
            0,
        );
    }
    if active && let Some(install) = native().and_then(|calls| calls.controller_install) {
        unsafe { install(Some(controller_received)) };
    }
    let previous = CONTROLLER_POLL_SCOPE.replace(INSIDE_ENGINE_TICK.get());
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::POLL_INPUT) };
    unsafe { forward(slate) };
    CONTROLLER_POLL_SCOPE.set(previous);
}

pub(super) unsafe extern "C" fn hooked_pump_messages(main_loop: u8) {
    let _lifetime = Guard::outer();
    if main_loop != 0
        && INSIDE_ENGINE_TICK.get()
        && INPUT_SOURCE_FRAME.get() != 0
        && !INPUT_SAMPLED.get()
    {
        INPUT_SAMPLED.set(true);
        cpu_event(
            installed(),
            INPUT_SOURCE_FRAME.get(),
            RSF_GAME_CPU_INPUT_SAMPLE,
            0,
            0,
        );
    }
    let forward: unsafe extern "C" fn(u8) = unsafe { original(hook::PUMP_MESSAGES) };
    unsafe { forward(main_loop) };
}

pub(super) unsafe extern "C" fn hooked_frame_sync(sync: *mut c_void, one_frame_lag: u8) {
    let _lifetime = Guard::outer();
    let engine_sync = game_module().wrapping_add(0x3a4_a6f8).cast::<c_void>();
    if sync == engine_sync
        && INSIDE_ENGINE_TICK.get()
        && INPUT_SOURCE_FRAME.get() != 0
        && SIMULATION_STARTED.get()
        && !SIMULATION_CLOSED.get()
    {
        SIMULATION_CLOSED.set(true);
        cpu_event(
            installed(),
            INPUT_SOURCE_FRAME.get(),
            RSF_GAME_CPU_SIMULATION_END,
            0,
            0,
        );
    }
    let forward: unsafe extern "C" fn(*mut c_void, u8) = unsafe { original(hook::FRAME_SYNC) };
    unsafe { forward(sync, one_frame_lag) };
}

pub(super) unsafe extern "C" fn hooked_engine_pacing(object: *mut c_void) {
    let _lifetime = Guard::outer();
    // This native time-update boundary does not report a poll. The input hooks report it.
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::ENGINE_PACING) };
    unsafe { forward(object) };
}

pub(super) unsafe extern "C" fn hooked_frame_task_construct(
    task: *mut c_void,
    completion: *mut c_void,
    prerequisites: i32,
) -> *mut c_void {
    let _lifetime = Guard::outer();
    let construct: unsafe extern "C" fn(*mut c_void, *mut c_void, i32) -> *mut c_void =
        unsafe { original(hook::FRAME_TASK_CONSTRUCT) };
    let result = unsafe { construct(task, completion, prerequisites) };
    let reserved = RESERVED_SOURCE_FRAME.get();
    if let Some(owner) = installed()
        && owner.active()
        && INSIDE_ENGINE_TICK.get()
        && reserved != 0
    {
        let mut bound = false;
        {
            let mut tables = owner.tables();
            if let Some(entry) = tables.frame_tasks.iter_mut().find(|entry| entry.task == 0) {
                entry.task = key(task);
                entry.source = reserved;
                owner.live_frame_tasks.fetch_add(1, Ordering::SeqCst);
                bound = true;
            }
        }
        if !bound {
            owner.deactivate();
            owner.log(c"AC7 frame identity refused: BeginFrame task capacity reached");
        }
    }
    result
}

pub(super) unsafe extern "C" fn hooked_frame_task_execute(
    task: *mut c_void,
    scratch: *mut c_void,
    thread: u32,
) {
    let _lifetime = Guard::outer();
    let saved = FRAME_TASK_SOURCE.replace(0);
    if let Some(owner) = installed() {
        let mut tables = owner.tables();
        if let Some(entry) = tables
            .frame_tasks
            .iter_mut()
            .find(|entry| entry.task == key(task))
        {
            FRAME_TASK_SOURCE.set(entry.source);
            entry.task = 0;
            entry.source = 0;
            owner.live_frame_tasks.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) =
        unsafe { original(hook::FRAME_TASK_EXECUTE) };
    unsafe { forward(task, scratch, thread) };
    FRAME_TASK_SOURCE.set(saved);
}

pub(super) unsafe extern "C" fn hooked_rhi_frame_begin(list: *mut c_void) {
    let _lifetime = Guard::outer();
    let source = match FRAME_TASK_SOURCE.get() {
        0 if INSIDE_ENGINE_TICK.get() => RESERVED_SOURCE_FRAME.get(),
        source => source,
    };
    if let Some(owner) = installed()
        && owner.active()
        && source != 0
    {
        let mut pass = scope();
        pass.role = RSF_GAME_RENDER_FRAME;
        pass.session_id = owner.options.session_id;
        pass.source_frame_id = source;
        let opened = if FULL_FRAME_TICKET.get().is_null() {
            open(owner, list, &pass)
        } else {
            None
        };
        match opened {
            Some(ticket) => FULL_FRAME_TICKET.set(ticket),
            None => {
                owner.deactivate();
                owner.log(c"AC7 full RHI frame refused: overlapping or invalid BeginFrame");
            }
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::RHI_FRAME_BEGIN) };
    unsafe { forward(list) };
}

pub(super) unsafe extern "C" fn hooked_rhi_frame_end(list: *mut c_void) {
    let _lifetime = Guard::outer();
    let ticket = FULL_FRAME_TICKET.get();
    if !ticket.is_null() {
        if close(ticket, list) {
            FULL_FRAME_TICKET.set(null_mut());
        } else if let Some(owner) = installed() {
            // Only the originating recording list can retire this ticket. Keep it reachable
            // while the renderer is deactivated and the EndFrame hook can still drain it.
            owner.deactivate();
            owner.log(c"AC7 full RHI frame refused: EndFrame could not queue");
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::RHI_FRAME_END) };
    unsafe { forward(list) };
}

pub(super) unsafe extern "C" fn hooked_renderer_retire(list: *mut c_void, renderer: *mut c_void) {
    let _lifetime = Guard::outer();
    let owner = installed();
    {
        let _producer = Guard::entry();
        if let Some(owner) = owner {
            owner
                .render_thread
                .store(super::current_thread(), Ordering::SeqCst);
            drain_retired(owner);
            // Remove the CPU key before native deletion can free/reuse its address. Queued
            // packets already own copied identity; late producers receive no association.
            let mut tables = owner.tables();
            if let Some(slot) = tables
                .identities
                .iter_mut()
                .find(|slot| slot.renderer == key(renderer))
            {
                *slot = RendererIdentity {
                    renderer: 0,
                    source: 0,
                    submission: 0,
                    viewport: 0,
                    after_simulation: false,
                    screen: RSF_SCREEN_UNKNOWN,
                    reset: 0,
                };
                owner.live_renderers.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { original(hook::RENDERER_RETIRE) };
    unsafe { forward(list, renderer) };
    {
        let _producer = Guard::entry();
        // Native task/RHI waits may release the last leases synchronously. The outer guard keeps
        // this module and its controller alive until the final render-owner drain returns.
        if let Some(owner) = owner {
            drain_retired(owner);
            retire_renderer_uniforms(owner, renderer, list);
        }
    }
}

pub(super) unsafe extern "C" fn hooked_render_family(list: *mut c_void, renderer: *mut c_void) {
    let _lifetime = Guard::outer();
    let owner = installed();
    let mut ticket = None;
    if let Some(owner) = owner
        && owner.active()
    {
        let mut pass = scope();
        pass.role = RSF_GAME_RENDER_SUBMISSION;
        pass.session_id = owner.options.session_id;
        pass.family_key = key(renderer).wrapping_add(0x10);
        pass.pass_key = key(renderer);
        pass.flags = RSF_GAME_RENDER_PRIMARY;
        renderer_identity(owner, &mut pass);
        // The scope contract requires the same concrete primary view used by SR. A family
        // key alone refuses open and cannot produce a completion marker for the CPU join.
        read(key(renderer), 0xb8, &mut pass.view_key);
        if pass.view_key != 0 {
            read(pass.view_key, 0x70, &mut pass.render_rect);
            read(pass.view_key, 0x80, &mut pass.output_rect);
        }
        let mut native_frame = 0u32;
        if read(pass.family_key, 0x68, &mut native_frame) {
            pass.native_frame = u64::from(native_frame);
        }
        if pass.source_frame_id != 0
            && pass.viewport_key != 0
            && pass.flags & RSF_GAME_RENDER_AFTER_SIMULATION != 0
            && source_primary_renderer(owner, renderer)
        {
            ticket = open(owner, list, &pass);
            if ticket.is_none() {
                owner.log(c"AC7 submission scope refused: no RHI completion marker will be queued");
            }
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { original(hook::RENDER_FAMILY) };
    unsafe { forward(list, renderer) };
    if let (Some(ticket), Some(owner)) = (ticket, owner)
        && !close(ticket, list)
    {
        owner.deactivate();
        owner.log(c"AC7 submission source deactivated: RHI scope could not close");
    }
}
