// SPDX-License-Identifier: MIT
//! CPU and render facts for the matched Skyrim executable. The native detours own the
//! callback leases and original calls. The host receives these facts at the native boundaries.

use crate::platform::{self, Platform};
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_EVENT_VERSION_SOURCE, RSF_GAME_CPU_FRAME_END,
    RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_UNFRAMED,
    RSF_GAME_CPU_RENDER_SKIPPED, RSF_GAME_INPUT_CONTROLLER, RSF_GAME_INPUT_KEYBOARD,
    RSF_GAME_INPUT_MOUSE, RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_PRIMARY, rsf_game_cpu_event, rsf_game_cpu_source_event, rsf_game_render_pass,
    rsf_screen_class,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

// The GameSDK Rust header mirror omits the cast-valued C screen constants.
const SCREEN_UNKNOWN: rsf_screen_class = 0;
const SCREEN_MENU: rsf_screen_class = 1;
const SCREEN_LOADING: rsf_screen_class = 7;
const SCREEN_GAMEPLAY: rsf_screen_class = 8;

const WORLD_RETURN: usize = 0x65_6e32;
const MAIN: usize = 0x323_0530;
const UI: usize = 0x219_e5c0;
const UI_NAMES: usize = 0x21a_05c8;
const PLAYER: usize = 0x323_0778;
const MANAGER: usize = 0x322_0a90;
const CAMERA_ROOT: usize = 0x323_0740;
const NATIVE_FRAME: usize = 0x333_63f4;
const ACTIVE_WINDOW: usize = 0x333_0198;
const MENU_OPEN: usize = 0x116_9530;
const FROZEN_BACKGROUND: usize = 0x116_9590;
const LOADING_NAME_OFFSET: usize = 0xe8;
const MAIN_NAME_OFFSET: usize = 0xa8;
const MENU_QUERIES: [(usize, [u8; 19]); 2] = [
    (
        MENU_OPEN,
        [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x40, 0x48, 0xc7, 0x44, 0x24, 0x20, 0xfe, 0xff, 0xff,
            0xff, 0x33, 0xc0, 0x48, 0x89,
        ],
    ),
    (
        FROZEN_BACKGROUND,
        [
            0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x6c, 0x24, 0x18, 0x48, 0x89, 0x74, 0x24,
            0x20, 0x57, 0x41, 0x56, 0x41,
        ],
    ),
];

static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
static NEXT_RENDER_SCOPE: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
#[derive(Default)]
struct ScreenFacts {
    readable: bool,
    owned_main: bool,
    primary_world: bool,
    loading_menu: bool,
    main_menu: bool,
    frozen_background: bool,
    pause_count: u32,
    requested_mode: u32,
    running_mode: u32,
    freeze_time: bool,
    parent_cell: bool,
}

/// `rsf_skyrim_cpu_render_state`, lent by the native Main tick on its own thread.
#[repr(C)]
struct RenderState {
    scope: u64,
    screen: rsf_screen_class,
    primary_world_entries: u32,
    primary_count: u32,
    render_entered: bool,
    simulation_ended: bool,
    sampled: bool,
}

const _: () = {
    assert!(size_of::<ScreenFacts>() == 24);
    assert!(std::mem::offset_of!(ScreenFacts, pause_count) == 8);
    assert!(size_of::<RenderState>() == 24);
    assert!(std::mem::offset_of!(RenderState, render_entered) == 20);
};

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_next_source() -> u64 {
    NEXT_SOURCE.fetch_add(1, Ordering::SeqCst)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_classify_screen(facts: &ScreenFacts) -> rsf_screen_class {
    if !facts.readable || !facts.owned_main {
        return SCREEN_UNKNOWN;
    }
    if facts.loading_menu {
        return SCREEN_LOADING;
    }
    if facts.main_menu || facts.pause_count != 0 || facts.frozen_background {
        return SCREEN_MENU;
    }
    if !facts.primary_world
        || facts.freeze_time
        || !facts.parent_cell
        || facts.requested_mode > 1
        || facts.running_mode > 1
    {
        return SCREEN_UNKNOWN;
    }
    SCREEN_GAMEPLAY
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_rendered_source_eligible(
    active_scope: bool,
    primary_world_entries: u32,
    screen: rsf_screen_class,
) -> bool {
    active_scope && primary_world_entries == 1 && screen == SCREEN_GAMEPLAY
}

#[derive(PartialEq, Eq)]
struct NativeScreen {
    main: usize,
    ui: usize,
    names: usize,
    loading_name: usize,
    main_name: usize,
    player: usize,
    manager: usize,
    cell: usize,
    pause: u32,
    freeze: u8,
    requested: u32,
    running: u32,
}

fn read_u32(platform: &Platform, address: usize) -> Option<u32> {
    let mut bytes = [0; 4];
    platform
        .read(address, &mut bytes)
        .then(|| u32::from_le_bytes(bytes))
}

impl NativeScreen {
    fn read(platform: &Platform, base: usize) -> Option<Self> {
        let nonzero = |address| platform.read_usize(address).filter(|value| *value != 0);
        let main = nonzero(base + MAIN)?;
        let ui = nonzero(base + UI)?;
        let names = nonzero(base + UI_NAMES)?;
        let loading_name = nonzero(names.checked_add(LOADING_NAME_OFFSET)?)?;
        let main_name = nonzero(names.checked_add(MAIN_NAME_OFFSET)?)?;
        let player = nonzero(base + PLAYER)?;
        let manager = nonzero(base + MANAGER)?;
        let cell = platform.read_usize(player.checked_add(0x60)?)?;
        let pause = read_u32(platform, ui.checked_add(0x160)?)?;
        let mut freeze = [0];
        if !platform.read(main.checked_add(0x16)?, &mut freeze) {
            return None;
        }
        Some(Self {
            main,
            ui,
            names,
            loading_name,
            main_name,
            player,
            manager,
            cell,
            pause,
            freeze: freeze[0],
            requested: read_u32(platform, manager.checked_add(0x68)?)?,
            running: read_u32(platform, manager.checked_add(0x6c)?)?,
        })
    }
}

type MenuQueries =
    unsafe extern "C" fn(usize, usize, usize, usize, usize, &mut ScreenFacts) -> bool;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cpu_bad_screen_query(table: *const Platform, base: usize) -> u32 {
    // The upstream CPU prepare runs before session registration of this same platform.
    let Some(platform) = (unsafe { platform::inspect(table) }) else {
        return MENU_OPEN as u32;
    };
    MENU_QUERIES
        .iter()
        .find(|(rva, bytes)| !platform.matches(base + rva, bytes))
        .map_or(0, |(rva, _)| *rva as u32)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_screen(
    main: usize,
    primary: bool,
    menus: MenuQueries,
) -> rsf_screen_class {
    let Some(platform) = platform::get() else {
        return SCREEN_UNKNOWN;
    };
    let base = platform.main_module();
    let Some(before) = NativeScreen::read(platform, base).filter(|state| state.main == main) else {
        return SCREEN_UNKNOWN;
    };
    let mut facts = ScreenFacts::default();
    // Native menu calls take the engine's own map synchronization and borrow its names.
    if !unsafe {
        menus(
            base + MENU_OPEN,
            base + FROZEN_BACKGROUND,
            before.ui,
            before.names + LOADING_NAME_OFFSET,
            before.names + MAIN_NAME_OFFSET,
            &mut facts,
        )
    } || NativeScreen::read(platform, base).as_ref() != Some(&before)
    {
        return SCREEN_UNKNOWN;
    }
    facts.readable = true;
    facts.owned_main = true;
    facts.primary_world = primary;
    facts.pause_count = before.pause;
    facts.requested_mode = before.requested;
    facts.running_mode = before.running;
    facts.freeze_time = before.freeze != 0;
    facts.parent_cell = before.cell != 0;
    rsf_skyrim_classify_screen(&facts)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_begin_render(
    state: &mut RenderState,
    screen: rsf_screen_class,
) -> bool {
    state.screen = screen;
    state.scope = NEXT_RENDER_SCOPE.fetch_add(1, Ordering::SeqCst);
    state.render_entered = true;
    !std::mem::replace(&mut state.simulation_ended, true)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_end_render(state: &mut RenderState, previous_scope: u64) {
    state.scope = previous_scope;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_begin_world(
    state: &mut RenderState,
    pass: &rsf_game_render_pass,
    base: usize,
    caller: usize,
    output: u32,
) -> bool {
    if base.checked_add(WORLD_RETURN) != Some(caller)
        || output != 0
        || pass.source_frame_id == 0
        || pass.view_key == 0
        || pass.swapchain.is_null()
    {
        return false;
    }
    state.primary_world_entries = state.primary_world_entries.wrapping_add(1);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_world_screen(state: &mut RenderState, screen: rsf_screen_class) {
    state.screen = if state.primary_world_entries == 1 {
        screen
    } else {
        SCREEN_UNKNOWN
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_end_world(state: &mut RenderState) {
    state.primary_count = state.primary_count.wrapping_add(1);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_render_identity(
    state: &RenderState,
    pass: &mut rsf_game_render_pass,
    source: u64,
    active: bool,
) -> bool {
    if state.scope == 0 || !state.simulation_ended {
        return false;
    }
    pass.source_frame_id = source;
    pass.submission_id = state.scope;
    pass.scope_id = state.scope;
    pass.screen = state.screen;
    pass.flags |= RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
    if rsf_skyrim_rendered_source_eligible(active, state.primary_world_entries, state.screen) {
        pass.flags &= !RSF_GAME_RENDER_NO_FG;
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_render_fact(pass: &mut rsf_game_render_pass, session: u64) {
    let Some(platform) = platform::get() else {
        return;
    };
    let base = platform.main_module();
    pass.session_id = session;
    pass.flags |= RSF_GAME_RENDER_PRIMARY;
    pass.native_frame = u64::from(read_u32(platform, base + NATIVE_FRAME).unwrap_or(0));
    let camera = platform
        .read_usize(base + CAMERA_ROOT)
        .filter(|root| *root != 0)
        .and_then(|root| platform.read_usize(root.checked_add(296)?))
        .unwrap_or(0) as u64;
    pass.family_key = camera;
    pass.view_key = camera;
    pass.history_key = camera;
    if let Some(window) = platform
        .read_usize(base + ACTIVE_WINDOW)
        .filter(|window| *window != 0)
    {
        pass.viewport_key = window as u64;
        pass.window_key = window as u64;
        pass.rhi_viewport_key = window as u64;
        let swapchain = window
            .checked_add(0x18)
            .and_then(|slot| platform.read_usize(slot))
            .unwrap_or(0);
        pass.swapchain = std::ptr::with_exposed_provenance_mut(swapchain);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_sample_tick(state: &mut RenderState) -> bool {
    !std::mem::replace(&mut state.sampled, true)
}

fn cpu_event(
    stage: u32,
    session: u64,
    source: u64,
    qpc: u64,
    frequency: u64,
) -> rsf_game_cpu_event {
    rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage,
        session_id: session,
        source_frame_id: source,
        timestamp_qpc: qpc,
        qpc_frequency: frequency,
        render_expected: 0,
        input_kind: 0,
        message_id: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: 0,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_frame_event(
    out: &mut rsf_game_cpu_event,
    state: &RenderState,
    stage: u32,
    session: u64,
    source: u64,
    qpc: u64,
    frequency: u64,
) {
    let mut flags = RSF_GAME_CPU_INPUT_KEYED;
    if stage == RSF_GAME_CPU_FRAME_END && !state.render_entered {
        flags |= RSF_GAME_CPU_RENDER_SKIPPED;
    }
    *out = rsf_game_cpu_event {
        render_expected: state.primary_count,
        flags,
        ..cpu_event(stage, session, source, qpc, frequency)
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_source_event(
    out: &mut rsf_game_cpu_source_event,
    stage: u32,
    session: u64,
    source: u64,
    qpc: u64,
    frequency: u64,
    predecessor: u64,
    generation: u64,
    flags: u32,
    kinds: u32,
    reason: u32,
    poll_begin: u64,
) {
    *out = rsf_game_cpu_source_event {
        header: rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_source_event>() as u32,
            event_version: RSF_GAME_CPU_EVENT_VERSION_SOURCE,
            input_kind: kinds,
            flags,
            ..cpu_event(stage, session, source, qpc, frequency)
        },
        predecessor_source_id: predecessor,
        native_generation: generation,
        reason,
        reserved: 0,
        poll_begin_qpc: poll_begin,
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_input_event(
    out: &mut rsf_game_cpu_event,
    session: u64,
    kinds: u32,
    qpc: u64,
    frequency: u64,
) {
    *out = rsf_game_cpu_event {
        input_kind: kinds,
        flags: RSF_GAME_CPU_INPUT_UNFRAMED,
        ..cpu_event(RSF_GAME_CPU_INPUT_EVENT, session, 0, qpc, frequency)
    };
}

type ReadNative = unsafe extern "C" fn(usize, *mut c_void, usize) -> bool;

#[repr(C)]
#[derive(Default)]
struct NativeInputEvent {
    _vtable: usize,
    device: u32,
    kind: u32,
    next: usize,
}

const _: () = assert!(size_of::<NativeInputEvent>() == 0x18);

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_input_kinds(list: usize, read: ReadNative) -> u32 {
    let mut next = 0usize;
    if list == 0
        || !unsafe {
            read(
                list,
                std::ptr::from_mut(&mut next).cast(),
                size_of::<usize>(),
            )
        }
    {
        return 0;
    }
    let mut kinds = 0;
    // The native dispatch only lends these headers until the queue is cleared. A
    // cyclic or excessive list refuses all activity, including the readable prefix.
    for _ in 0..128 {
        if next == 0 {
            break;
        }
        let mut event = NativeInputEvent::default();
        if !unsafe {
            read(
                next,
                std::ptr::from_mut(&mut event).cast(),
                size_of::<NativeInputEvent>(),
            )
        } {
            return 0;
        }
        kinds |= match (event.device, event.kind) {
            (0 | 3, 0 | 2) => RSF_GAME_INPUT_KEYBOARD,
            (1, 0 | 1) => RSF_GAME_INPUT_MOUSE,
            (2, 0 | 3) => RSF_GAME_INPUT_CONTROLLER,
            _ => 0,
        };
        next = event.next;
    }
    if next == 0 { kinds } else { 0 }
}
