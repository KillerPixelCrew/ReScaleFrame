// SPDX-License-Identifier: MIT
use super::*;
use crate::platform::fake;
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_INPUT_EVENT,
    RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_SAMPLE, RSF_GAME_CPU_WINDOW_MESSAGE,
    rsf_game_cpu_event,
};
use std::ffi::c_void;
use std::ptr;

const ENGINE: usize = 0x100;
const OTHER: usize = 0x200;

fn handle(value: usize) -> *mut c_void {
    ptr::without_provenance_mut(value)
}

fn event(stage: u32, session: u64) -> rsf_game_cpu_event {
    rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage,
        session_id: session,
        source_frame_id: 9,
        timestamp_qpc: 0,
        qpc_frequency: 0,
        render_expected: 0,
        input_kind: 0,
        message_id: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: RSF_GAME_CPU_INPUT_KEYED,
    }
}

fn admitted() -> Window {
    let window = Window::new();
    window.bind_cpu_service(Some(&fake::D3D12_CPU));
    assert!(window.accept(handle(ENGINE)));
    fake::take_window_calls();
    window
}

#[test]
fn admits_the_first_engine_window_of_this_process() {
    let window = Window::new();
    window.bind_cpu_service(Some(&fake::D3D12_CPU));
    assert!(!window.accept(ptr::null_mut()));
    assert!(fake::take_window_calls().is_empty());
    fake::IDENTITY.set((11, 10, "Engine"));
    assert!(!window.accept(handle(ENGINE)));
    fake::IDENTITY.set((0, 10, "Engine"));
    assert!(!window.accept(handle(ENGINE)));
    for class in ["Engin", "EngineX", "engine", ""] {
        fake::IDENTITY.set((10, 10, class));
        assert!(!window.accept(handle(ENGINE)), "{class}");
    }
    assert!(window.input().is_null());
    fake::IDENTITY.set((10, 10, "Engine"));
    assert!(window.accept(handle(ENGINE)));
    assert!(window.accept(handle(ENGINE)));
    // Another Engine window is refused while the first stays admitted.
    assert!(!window.accept(handle(OTHER)));
    assert_eq!(window.input().addr(), ENGINE);
}

#[test]
fn frame_begin_attaches_the_procedure_once() {
    fake::PROCEDURE.set(fake::ENGINE_PROCEDURE);
    let window = Window::new();
    window.bind_cpu_service(Some(&fake::D3D12_CPU));
    // Nothing is attached before a window is admitted.
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 5));
    assert!(fake::take_window_calls().is_empty());
    assert!(window.accept(handle(ENGINE)));
    fake::take_window_calls();
    fake::take_cpu_events();
    window.cpu(&event(RSF_GAME_CPU_INPUT_SAMPLE, 5));
    assert!(fake::take_window_calls().is_empty());
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 5));
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 5));
    assert_eq!(
        fake::take_window_calls(),
        [("set", procedure_address() as u64)]
    );
    assert_eq!(fake::PROCEDURE.get(), procedure_address());
    // Each event reaches the D3D12 backend unchanged, in order.
    let reported = |stage| (false, stage, 5, 0, RSF_GAME_CPU_INPUT_KEYED);
    let begin = reported(RSF_GAME_CPU_FRAME_BEGIN);
    assert_eq!(
        fake::take_cpu_events(),
        [reported(RSF_GAME_CPU_INPUT_SAMPLE), begin, begin]
    );
}

#[test]
fn a_refused_attachment_is_retried_at_the_next_frame_begin() {
    fake::PROCEDURE.set(fake::ENGINE_PROCEDURE);
    let window = admitted();
    fake::PROCEDURE_REFUSED.set(true);
    window.bind_cpu_service(Some(&fake::VK_CPU));
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 5));
    fake::PROCEDURE_REFUSED.set(false);
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 5));
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 5));
    let set = ("set", procedure_address() as u64);
    assert_eq!(fake::take_window_calls(), [set, set]);
    // A Vulkan session's events reach the Vulkan latency owner.
    assert!(fake::take_cpu_events().iter().all(|event| event.0));
}

#[test]
fn messages_report_the_session_and_reach_the_replaced_procedure() {
    fake::PROCEDURE.set(fake::ENGINE_PROCEDURE);
    let window = admitted();
    window.bind_cpu_service(Some(&fake::VK_CPU));
    // Before attachment the default procedure answers.
    assert_eq!(window.message(handle(ENGINE), 0x100, 0, 0), fake::FORWARDED);
    assert_eq!(fake::take_window_calls()[0], ("forward", 0));
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 7));
    fake::take_cpu_events();
    fake::take_window_calls();
    assert_eq!(window.message(handle(ENGINE), 0x201, 1, 2), fake::FORWARDED);
    assert_eq!(
        fake::take_cpu_events(),
        [(
            true,
            RSF_GAME_CPU_INPUT_EVENT,
            7,
            0x201,
            RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE
        )]
    );
    assert_eq!(
        fake::take_window_calls(),
        [
            ("forward", fake::ENGINE_PROCEDURE as u64),
            ("forward_window", ENGINE as u64),
            ("forward_message", 0x201),
        ]
    );
}

#[test]
fn release_restores_only_its_own_procedure() {
    fake::PROCEDURE.set(fake::ENGINE_PROCEDURE);
    let window = admitted();
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 7));
    fake::take_window_calls();
    window.release();
    assert_eq!(
        fake::take_window_calls(),
        [("get", 0), ("set", fake::ENGINE_PROCEDURE as u64)]
    );
    assert_eq!(fake::PROCEDURE.get(), fake::ENGINE_PROCEDURE);
    assert!(window.input().is_null());
    // A released owner admits and attaches again.
    assert!(window.accept(handle(OTHER)));
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 8));
    assert_eq!(fake::PROCEDURE.get(), procedure_address());
}

#[test]
fn a_refused_restore_keeps_the_previous_procedure_available() {
    fake::PROCEDURE.set(fake::ENGINE_PROCEDURE);
    let window = admitted();
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 7));
    fake::take_window_calls();
    fake::PROCEDURE_REFUSED.set(true);
    window.release();
    fake::PROCEDURE_REFUSED.set(false);
    assert_eq!(
        fake::take_window_calls(),
        [("get", 0), ("set", fake::ENGINE_PROCEDURE as u64)]
    );
    assert_eq!(fake::PROCEDURE.get(), procedure_address());
    assert!(window.input().is_null());
    window.message(handle(ENGINE), 0x100, 0, 0);
    assert_eq!(
        fake::take_window_calls()[0],
        ("forward", fake::ENGINE_PROCEDURE as u64)
    );
}

#[test]
fn release_under_a_foreign_subclass_keeps_forwarding() {
    fake::PROCEDURE.set(fake::ENGINE_PROCEDURE);
    let window = admitted();
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 7));
    fake::PROCEDURE.set(0x6000);
    fake::take_window_calls();
    fake::take_cpu_events();
    window.release();
    assert_eq!(fake::take_window_calls(), [("get", 0)]);
    assert!(window.input().is_null());
    // Retired windows keep forwarding without sending inputs to the active session.
    window.message(handle(ENGINE), 0x100, 0, 0);
    assert!(fake::take_cpu_events().is_empty());
    assert_eq!(
        fake::take_window_calls()[0],
        ("forward", fake::ENGINE_PROCEDURE as u64)
    );
    fake::PROCEDURE.set(0x7000);
    assert!(window.accept(handle(OTHER)));
    window.cpu(&event(RSF_GAME_CPU_FRAME_BEGIN, 8));
    assert_eq!(
        fake::take_window_calls(),
        [
            ("identity", OTHER as u64),
            ("set", procedure_address() as u64)
        ]
    );
    fake::take_cpu_events();
    window.message(handle(ENGINE), 0x100, 0, 0);
    assert_eq!(
        fake::take_window_calls()[0],
        ("forward", fake::ENGINE_PROCEDURE as u64)
    );
    assert!(fake::take_cpu_events().is_empty());
    window.message(handle(OTHER), 0x100, 0, 0);
    assert_eq!(fake::take_window_calls()[0], ("forward", 0x7000));
    assert_eq!(fake::take_cpu_events()[0].2, 8);
}

#[test]
fn pings_and_present_markers_use_the_admitted_window() {
    let window = Window::new();
    window.bind_cpu_service(Some(&fake::D3D12_CPU));
    assert_eq!(window.latency_ping(), 1);
    assert!(!window.present_marker(true, ENGINE as u64, 1));
    assert!(window.accept(handle(ENGINE)));
    fake::take_window_calls();
    assert_eq!(window.latency_ping(), 1);
    window.bind_cpu_service(Some(&fake::VK_CPU));
    assert_eq!(window.latency_ping(), 2);
    assert!(!window.present_marker(false, ENGINE as u64, 1));
    assert!(!window.present_marker(true, 0, 1));
    assert!(!window.present_marker(true, OTHER as u64, 1));
    assert!(window.present_marker(true, ENGINE as u64, 1));
    assert!(window.present_marker(true, ENGINE as u64, 0));
    assert_eq!(
        fake::take_window_calls(),
        [
            ("ping", ENGINE as u64),
            ("vulkan_ping", ENGINE as u64),
            ("marker", 1),
            ("marker", 0),
        ]
    );
}

#[test]
fn explicit_present_forwards_one_matching_source_even_after_window_release() {
    let window = admitted();
    window.bind_cpu_service(Some(&fake::VK_CPU));
    let mut pass = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        role: RSF_GAME_RENDER_PRESENT,
        session_id: 7,
        source_frame_id: 9,
        window_key: OTHER as u64,
        ..Default::default()
    };
    assert!(!window.execution(&pass, 1));
    pass.window_key = ENGINE as u64;
    assert!(window.execution(&pass, 1));
    assert!(!window.execution(&pass, 1));
    window.release();
    pass.source_frame_id = 8;
    assert!(!window.execution(&pass, 0));
    pass.source_frame_id = 9;
    assert!(window.execution(&pass, 0));
    assert_eq!(
        fake::take_window_calls(),
        [
            ("execution_role", RSF_GAME_RENDER_PRESENT.into()),
            ("execution_source", 9),
            ("execution_window", ENGINE as u64),
            ("execution_begin", 1),
            ("execution_role", RSF_GAME_RENDER_PRESENT.into()),
            ("execution_source", 9),
            ("execution_window", ENGINE as u64),
            ("execution_begin", 0),
        ]
    );
}
