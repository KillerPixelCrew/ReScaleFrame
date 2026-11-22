// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::{RSF_GAME_CPU_SOURCE_PRODUCED, RSF_GAME_CPU_SOURCE_SAMPLED};
use std::ptr;

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_input_fixture(read: ReadNative) -> i32 {
    let mut failures = 0;
    let mut check = |passed: bool, label: &str| {
        if !passed {
            eprintln!("FAIL {label}");
            failures += 1;
        }
    };
    render_scope(&mut check);
    input_headers(read, &mut check);
    i32::from(failures != 0)
}

fn render_scope(check: &mut impl FnMut(bool, &str)) {
    let base = 0x1000_0000;
    let caller = base + WORLD_RETURN;
    let (session, source) = (9, 17);
    // These C records contain integers, booleans and raw pointers. Zero is their empty state.
    let (mut state, mut pass, mut event, mut sample): (
        RenderState,
        rsf_game_render_pass,
        rsf_game_cpu_event,
        rsf_game_cpu_source_event,
    ) = unsafe { std::mem::zeroed() };
    let (mut outer, mut nested) = (RenderReceipt::default(), RenderReceipt::default());
    rsf_skyrim_cpu_frame_event(
        &mut event,
        &state,
        RSF_GAME_CPU_FRAME_END,
        session,
        source,
        20,
        50,
    );
    check(
        event.flags == (RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_RENDER_SKIPPED)
            && event.render_expected == 0,
        "skipped render closes the CPU frame",
    );
    check(
        !rsf_skyrim_cpu_render_identity(&state, &mut pass, source, true),
        "identity requires RenderEntry",
    );
    check(
        rsf_skyrim_cpu_begin_render(&mut state, SCREEN_UNKNOWN, &mut outer),
        "first RenderEntry closes simulation",
    );
    let scope = state.scope;
    check(
        rsf_skyrim_cpu_render_identity(&state, &mut pass, source, true)
            && scope != 0
            && pass.scope_id == scope
            && pass.submission_id == scope
            && pass.source_frame_id == source
            && pass.flags & RSF_GAME_RENDER_NO_FG != 0,
        "frame is scoped and refuses FG before primary World",
    );
    let mut chain = 0u32;
    pass.swapchain = ptr::from_mut(&mut chain).cast();
    pass.view_key = 1;
    check(
        !rsf_skyrim_cpu_begin_world(&mut state, &pass, base, caller + 1, 0)
            && !rsf_skyrim_cpu_begin_world(&mut state, &pass, base, caller, 1)
            && state.primary_world_entries == 0,
        "foreign caller and non-primary output refuse admission",
    );
    check(
        rsf_skyrim_cpu_begin_world(&mut state, &pass, base, caller, 0),
        "mapped World caller enters the source",
    );
    rsf_skyrim_cpu_world_screen(&mut state, SCREEN_GAMEPLAY);
    check(
        rsf_skyrim_cpu_render_identity(&state, &mut pass, source, true)
            && pass.flags & RSF_GAME_RENDER_NO_FG == 0
            && pass.screen == SCREEN_GAMEPLAY
            && pass.scope_id == scope,
        "gameplay World admits its render scope",
    );
    check(
        rsf_skyrim_cpu_render_identity(&state, &mut pass, source, false)
            && pass.flags & RSF_GAME_RENDER_NO_FG != 0,
        "inactive observer labels the scope and refuses FG",
    );
    rsf_skyrim_cpu_end_world(&mut state);
    check(
        rsf_skyrim_cpu_begin_world(&mut state, &pass, base, caller, 0),
        "duplicate World is observed",
    );
    rsf_skyrim_cpu_world_screen(&mut state, SCREEN_GAMEPLAY);
    check(
        rsf_skyrim_cpu_render_identity(&state, &mut pass, source, true)
            && pass.flags & RSF_GAME_RENDER_NO_FG != 0
            && pass.screen == SCREEN_UNKNOWN,
        "duplicate World withdraws gameplay admission",
    );
    rsf_skyrim_cpu_end_world(&mut state);
    check(
        !rsf_skyrim_cpu_begin_render(&mut state, SCREEN_LOADING, &mut nested)
            && state.scope != scope,
        "nested RenderEntry starts a distinct scope without repeating simulation end",
    );
    rsf_skyrim_cpu_end_render(&mut state, &nested);
    check(
        rsf_skyrim_cpu_render_identity(&state, &mut pass, source, true)
            && pass.scope_id == scope
            && pass.screen == SCREEN_UNKNOWN,
        "nested RenderEntry restores its parent receipt",
    );
    rsf_skyrim_cpu_end_render(&mut state, &outer);
    check(
        !rsf_skyrim_cpu_render_identity(&state, &mut pass, source, true),
        "closed render has no live identity",
    );
    rsf_skyrim_cpu_frame_event(
        &mut event,
        &state,
        RSF_GAME_CPU_FRAME_END,
        session,
        source,
        30,
        50,
    );
    check(
        event.render_expected == 2
            && event.flags == RSF_GAME_CPU_INPUT_KEYED
            && event.session_id == session
            && event.source_frame_id == source
            && event.timestamp_qpc == 30
            && event.qpc_frequency == 50,
        "frame end counts returned Worlds and preserves the callback clock",
    );
    check(
        rsf_skyrim_cpu_sample_tick(&mut state) && !rsf_skyrim_cpu_sample_tick(&mut state),
        "tick reports the unreserved native sample once",
    );
    rsf_skyrim_cpu_source_event(
        &mut sample,
        RSF_GAME_CPU_SOURCE_SAMPLED,
        session,
        source,
        30,
        50,
        0,
        8,
        RSF_GAME_CPU_SOURCE_PRODUCED,
        RSF_GAME_INPUT_MOUSE,
        0,
        20,
    );
    check(
        sample.header.struct_size as usize == size_of_val(&sample)
            && sample.header.event_version == RSF_GAME_CPU_EVENT_VERSION_SOURCE
            && sample.header.stage == RSF_GAME_CPU_SOURCE_SAMPLED
            && sample.header.source_frame_id == source
            && sample.header.timestamp_qpc == 30
            && sample.poll_begin_qpc == 20
            && sample.native_generation == 8
            && sample.header.input_kind == RSF_GAME_INPUT_MOUSE
            && sample.header.flags == RSF_GAME_CPU_SOURCE_PRODUCED,
        "sample receipt preserves both native timestamps",
    );
}

fn input_headers(read: ReadNative, check: &mut impl FnMut(bool, &str)) {
    let mut event = NativeInputEvent::default();
    let classify = |list: &usize| rsf_skyrim_cpu_input_kinds(ptr::from_ref(list) as usize, read);
    let mut list = ptr::from_mut(&mut event) as usize;
    for (device, kind, expected, label) in [
        (0, 0, RSF_GAME_INPUT_KEYBOARD, "keyboard button"),
        (3, 2, RSF_GAME_INPUT_KEYBOARD, "virtual keyboard character"),
        (1, 1, RSF_GAME_INPUT_MOUSE, "mouse movement"),
        (2, 3, RSF_GAME_INPUT_CONTROLLER, "controller thumbstick"),
        (2, 4, 0, "device connection"),
        (99, 0, 0, "unknown device"),
        (0, 1, 0, "mismatched device and movement type"),
    ] {
        event.device = device;
        event.kind = kind;
        check(classify(&list) == expected, label);
    }
    let mut controller = NativeInputEvent {
        device: 2,
        ..NativeInputEvent::default()
    };
    let mouse = NativeInputEvent {
        device: 1,
        next: ptr::from_mut(&mut controller) as usize,
        ..NativeInputEvent::default()
    };
    event.kind = 0;
    event.next = ptr::from_ref(&mouse) as usize;
    check(
        classify(&list) == 7,
        "mixed list aggregates delivered kinds",
    );
    controller.next = list;
    check(classify(&list) == 0, "cyclic list refuses partial activity");
    event.next = 1;
    check(
        classify(&list) == 0,
        "unreadable node refuses partial activity",
    );
    check(
        rsf_skyrim_cpu_input_kinds(1, read) == 0,
        "unreadable head pointer",
    );
    check(
        rsf_skyrim_cpu_input_kinds(0, read) == 0,
        "absent head pointer",
    );
    list = 0;
    check(classify(&list) == 0, "empty list");
    let mut long_list = [const {
        NativeInputEvent {
            _vtable: 0,
            device: 0,
            kind: 0,
            next: 0,
        }
    }; 129];
    let start = long_list.as_mut_ptr();
    for (index, node) in long_list[..128].iter_mut().enumerate() {
        node.next = start.wrapping_add(index + 1) as usize;
    }
    list = start as usize;
    check(
        classify(&list) == 0,
        "excessive list refuses partial activity",
    );
    long_list[127].next = 0;
    check(
        classify(&list) == RSF_GAME_INPUT_KEYBOARD,
        "bounded list terminates successfully",
    );
}
