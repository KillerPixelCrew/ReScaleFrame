// SPDX-License-Identifier: MIT
use super::*;
use crate::control::default_settings;
use crate::platform::{Status, fake};
use rsf_game_sys::{RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_NONE, RSF_SR_SPATIAL};
use std::ptr;

const DEVICE: usize = 0x10;
const QUEUE: usize = 0x20;
const HUDLESS_LIST: usize = 0x900;
const CHAIN: usize = 0x77;

fn configure(control: &Control, change: impl FnOnce(&mut Settings)) {
    let mut settings = default_settings();
    change(&mut settings);
    assert_eq!(control.configure(&settings), 1);
}

fn pass(role: u32, frame: u64) -> rsf_game_render_pass {
    let mut pass = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        role,
        session_id: 42,
        source_frame_id: frame,
        native_frame: frame,
        screen: RSF_SCREEN_FLIGHT,
        color_input: ptr::without_provenance_mut(0x40),
        ..Default::default()
    };
    pass.camera.render_width = 1280;
    pass.camera.render_height = 720;
    pass.camera.output_width = 1920;
    pass.camera.output_height = 1080;
    pass
}

fn sr(frame: u64) -> rsf_game_render_pass {
    pass(RSF_GAME_RENDER_SR, frame)
}

/// The SR command list of a frame.
fn list(frame: u64) -> usize {
    0x100 + frame as usize
}

fn render_on(control: &Control, pass: &rsf_game_render_pass, commands: usize) -> c_int {
    control.d3d12.render(
        control,
        ptr::without_provenance_mut(DEVICE),
        ptr::without_provenance_mut(QUEUE),
        ptr::without_provenance_mut(commands),
        pass,
    )
}

fn render(control: &Control, pass: &rsf_game_render_pass) -> c_int {
    render_on(control, pass, list(pass.source_frame_id))
}

fn submit(control: &Control, lists: &[usize]) {
    control
        .d3d12
        .submitted(ptr::without_provenance_mut(QUEUE), lists);
}

fn status(control: &Control) -> Status {
    let mut value = fake::status_header();
    // SAFETY: a complete local record.
    assert_eq!(unsafe { control.status(&mut value) }, 1);
    value
}

#[test]
fn the_first_pass_binds_creates_the_auto_provider_and_evaluates_slot_zero() {
    let control = fake::boot_d3d12();
    assert_eq!(render(&control, &sr(1)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [("bind", 0), ("create", 1), ("wait", 0), ("evaluate", 0)]
    );
    let request = fake::EVALUATE.get().expect("evaluated");
    assert_eq!(request.screen, RSF_SCREEN_FLIGHT);
    assert_eq!(request.sharpening, fake::sharpening_defaults());
    let value = status(&control);
    assert_eq!(
        (value.requested_sr, value.active_sr, value.sr_result),
        (RSF_SR_AUTO, 1, backend_result::OK)
    );
    assert_eq!(
        (
            value.render_width,
            value.render_height,
            value.output_width,
            value.output_height
        ),
        (1280, 720, 1920, 1080)
    );
    assert_eq!(value.evaluated_frames, 1);
}

#[test]
fn failed_fence_creation_retries_before_publishing_the_binding() {
    let control = fake::boot_d3d12();
    fake::BIND.set(0);
    assert_eq!(render(&control, &sr(1)), 0);
    assert_eq!(fake::take_d3d12_calls(), [("bind", 0)]);
    assert_eq!(render(&control, &sr(2)), 0);
    assert_eq!(fake::take_d3d12_calls(), [("bind", 0)]);
    fake::BIND.set(1);
    assert_eq!(render(&control, &sr(2)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [("bind", 0), ("create", 1), ("wait", 0), ("evaluate", 0)]
    );
    let other = control.d3d12.render(
        &control,
        ptr::without_provenance_mut(DEVICE + 1),
        ptr::without_provenance_mut(QUEUE),
        ptr::without_provenance_mut(list(3)),
        &sr(3),
    );
    assert_eq!(other, 0);
    assert!(fake::take_d3d12_calls().is_empty());
}

#[test]
fn a_generation_change_drains_submitted_work_and_recreates() {
    let control = fake::boot_d3d12();
    assert_eq!(render(&control, &sr(1)), 1);
    submit(&control, &[list(1)]);
    fake::take_d3d12_calls();
    // FG options do not change the SR generation.
    configure(&control, |s| s.fg_mode = 1);
    assert_eq!(render(&control, &sr(2)), 1);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 0), ("evaluate", 1)]);
    configure(&control, |s| s.quality = 2);
    assert_eq!(render(&control, &sr(3)), 0);
    assert!(fake::take_d3d12_calls().is_empty());
    submit(&control, &[list(2)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 2)]);
    assert_eq!(render(&control, &sr(3)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [
            ("wait", 2),
            ("destroy", fake::context_handle(1)),
            ("create", 1),
            ("wait", 0),
            ("evaluate", 2)
        ]
    );
}

#[test]
fn a_provider_context_follows_its_output_but_not_its_input_extent() {
    let control = fake::boot_d3d12();
    let mut frame = sr(1);
    assert_eq!(render(&control, &frame), 1);
    fake::take_d3d12_calls();
    frame.source_frame_id = 2;
    frame.camera.render_width = 960;
    assert_eq!(render(&control, &frame), 1);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 0), ("evaluate", 1)]);
    submit(&control, &[list(1), list(2)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 1)]);
    frame.source_frame_id = 3;
    frame.camera.output_width = 2560;
    assert_eq!(render(&control, &frame), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [
            ("wait", 1),
            ("destroy", fake::context_handle(1)),
            ("create", 1),
            ("wait", 0),
            ("evaluate", 2)
        ]
    );
    assert_eq!(status(&control).output_width, 2560);
}

#[test]
fn spatial_uses_the_normalising_context_and_its_plan() {
    let control = fake::boot_d3d12();
    configure(&control, |s| s.sr_backend = RSF_SR_SPATIAL);
    let mut frame = sr(1);
    assert_eq!(render(&control, &frame), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [("bind", 0), ("create", 0), ("wait", 0), ("evaluate", 0)]
    );
    assert_eq!(status(&control).active_sr, RSF_SR_SPATIAL);
    submit(&control, &[list(1)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 1)]);
    frame.source_frame_id = 2;
    frame.camera.render_width = 960;
    fake::PLAN.set((960, 720));
    assert_eq!(render(&control, &frame), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [
            ("wait", 1),
            ("destroy", fake::context_handle(1)),
            ("create", 0),
            ("wait", 0),
            ("evaluate", 1)
        ]
    );
    assert_eq!(status(&control).render_width, 960);
}

#[test]
fn a_refused_provider_falls_back_and_reports_its_first_result() {
    let control = fake::boot_d3d12();
    configure(&control, |s| s.sr_backend = RSF_SR_DLSS);
    fake::REFUSED.set(Some((RSF_SR_DLSS, -7)));
    assert_eq!(render(&control, &sr(1)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [
            ("bind", 0),
            ("create", u64::from(RSF_SR_DLSS)),
            ("create", 0),
            ("wait", 0),
            ("evaluate", 0)
        ]
    );
    let value = status(&control);
    assert_eq!(
        (value.requested_sr, value.active_sr, value.sr_result),
        (RSF_SR_DLSS, 0, -7)
    );
}

#[test]
fn no_context_refuses_the_pass_before_a_slot_is_taken() {
    let control = fake::boot_d3d12();
    configure(&control, |s| s.sr_backend = RSF_SR_SPATIAL);
    fake::REFUSED.set(Some((RSF_SR_NONE, -3)));
    assert_eq!(render(&control, &sr(1)), 0);
    assert_eq!(fake::take_d3d12_calls(), [("bind", 0), ("create", 0)]);
    fake::REFUSED.set(None);
    assert_eq!(render(&control, &sr(2)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [("create", 0), ("wait", 0), ("evaluate", 0)]
    );
}

#[test]
fn each_slot_waits_for_its_own_last_submission() {
    let control = fake::boot_d3d12();
    for frame in 1..=3 {
        assert_eq!(render(&control, &sr(frame)), 1);
        submit(&control, &[list(frame)]);
    }
    fake::take_d3d12_calls();
    assert_eq!(render(&control, &sr(4)), 1);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 1), ("evaluate", 0)]);
    // An unmatched list or another queue signals nothing.
    submit(&control, &[list(9)]);
    control
        .d3d12
        .submitted(ptr::without_provenance_mut(QUEUE + 1), &[list(4)]);
    assert!(fake::take_d3d12_calls().is_empty());
    submit(&control, &[list(4)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 4)]);
    assert_eq!(render(&control, &sr(5)), 1);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 2), ("evaluate", 1)]);
}

#[test]
fn a_slot_stays_owned_until_its_recording_is_fenced() {
    let control = fake::boot_d3d12();
    for frame in 1..=3 {
        assert_eq!(render(&control, &sr(frame)), 1);
    }
    fake::take_d3d12_calls();
    assert_eq!(render(&control, &sr(4)), 0);
    assert_eq!(render(&control, &pass(RSF_GAME_RENDER_FINAL_SCENE, 4)), 0);
    assert_eq!(control.d3d12.shutdown(&control), 0);
    assert!(fake::take_d3d12_calls().is_empty());
    submit(&control, &[list(1)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 1)]);
    assert_eq!(render(&control, &sr(4)), 1);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 1), ("evaluate", 0)]);
}

#[test]
fn final_scene_refuses_an_sr_slot_whose_wait_failed() {
    let control = fake::boot_d3d12();
    for frame in 1..=3 {
        assert_eq!(render(&control, &sr(frame)), 1);
        submit(&control, &[list(frame)]);
    }
    fake::take_d3d12_calls();
    fake::WAIT.set(0);
    assert_eq!(render(&control, &sr(4)), 0);
    assert_eq!(
        render_on(
            &control,
            &pass(RSF_GAME_RENDER_FINAL_SCENE, 4),
            HUDLESS_LIST
        ),
        0
    );
    assert_eq!(fake::take_d3d12_calls(), [("wait", 1)]);
}

#[test]
fn the_final_scene_rides_the_last_slot_and_reports_its_submission() {
    let control = fake::boot_d3d12();
    assert_eq!(render(&control, &sr(1)), 1);
    assert_eq!(render(&control, &sr(2)), 1);
    fake::take_d3d12_calls();
    let scene = pass(RSF_GAME_RENDER_FINAL_SCENE, 2);
    assert_eq!(render_on(&control, &scene, HUDLESS_LIST), 1);
    assert_eq!(fake::take_d3d12_calls(), [("final_scene", 1)]);
    // One signal covers both lists; only the HUD-less record reaches FG.
    submit(&control, &[list(2), HUDLESS_LIST]);
    assert_eq!(
        fake::take_d3d12_calls(),
        [("signal", 1), ("fg_submitted", 2)]
    );
    // A refused signal keeps the frame 1 list pending.
    fake::SIGNAL.set(0);
    submit(&control, &[list(1)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 2)]);
    fake::SIGNAL.set(1);
    // The next signal on this queue covers the previously submitted list.
    submit(&control, &[list(9)]);
    assert_eq!(fake::take_d3d12_calls(), [("signal", 2)]);
}

#[test]
fn the_fg_provider_is_requested_only_from_an_owner_on_another_backend() {
    let control = fake::boot_d3d12();
    fake::PRESENT_OWNER.set(Some(1));
    assert_eq!(render(&control, &sr(1)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [
            ("bind", 0),
            ("present_request", 3),
            ("create", 1),
            ("wait", 0),
            ("evaluate", 0)
        ]
    );
    fake::PRESENT_OWNER.set(Some(3));
    assert_eq!(render(&control, &sr(2)), 1);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 0), ("evaluate", 1)]);
}

#[test]
fn only_gameplay_flight_passes_keep_the_flight_screen() {
    let mut settings = default_settings();
    assert_eq!(screen(&settings, RSF_SCREEN_FLIGHT), RSF_SCREEN_FLIGHT);
    assert_eq!(screen(&settings, RSF_SCREEN_UNKNOWN), RSF_SCREEN_MENU);
    settings.gameplay = 0;
    assert_eq!(screen(&settings, RSF_SCREEN_FLIGHT), RSF_SCREEN_MENU);
}

#[test]
fn input_refusals_are_logged_four_times_and_each_is_counted() {
    let control = fake::boot_d3d12();
    fake::EVALUATED.set(backend_result::INVALID_ARGUMENT);
    for frame in 1..=5 {
        assert_eq!(render(&control, &sr(frame)), 0);
        submit(&control, &[list(frame)]);
    }
    let logs = fake::take_d3d12_calls()
        .into_iter()
        .filter(|call| *call == ("refusal_log", 1280))
        .count();
    assert_eq!(logs, 4);
    let value = status(&control);
    assert_eq!(
        (value.refused_frames, value.active_sr, value.sr_result),
        (5, 0, backend_result::INVALID_ARGUMENT)
    );
}

#[test]
fn present_reuses_the_rendered_identity_or_reports_a_miss() {
    let control = fake::boot_d3d12();
    let chain = ptr::without_provenance_mut(CHAIN);
    control.d3d12.present(&control, chain);
    assert_eq!(fake::WINDOW.get(), None);
    control.d3d12.render_begin(&control, 1);
    assert_eq!(render(&control, &sr(1)), 1);
    control.d3d12.present(&control, chain);
    assert_eq!(
        fake::WINDOW.get(),
        Some((1, 42, RSF_GAME_RENDER_WINDOW, RSF_SCREEN_FLIGHT, CHAIN, 0))
    );
    assert_eq!(status(&control).active_sr, 1);
    control.d3d12.render_begin(&control, 2);
    control.d3d12.present(&control, chain);
    assert_eq!(
        fake::WINDOW.get(),
        Some((2, 42, RSF_GAME_RENDER_WINDOW, RSF_SCREEN_UNKNOWN, CHAIN, 0))
    );
    assert_eq!(status(&control).active_sr, 0);
}

#[test]
fn shutdown_drains_ends_the_fg_session_and_releases_the_context() {
    let control = fake::boot_d3d12();
    assert_eq!(render(&control, &sr(1)), 1);
    submit(&control, &[list(1)]);
    fake::take_d3d12_calls();
    fake::END_SESSION.set(0);
    assert_eq!(control.d3d12.shutdown(&control), 0);
    assert_eq!(fake::take_d3d12_calls(), [("wait", 1), ("end_session", 42)]);
    fake::END_SESSION.set(1);
    assert_eq!(control.d3d12.shutdown(&control), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [
            ("wait", 1),
            ("end_session", 42),
            ("release", fake::context_handle(1))
        ]
    );
    // The next pass recreates the context on the next slot.
    assert_eq!(render(&control, &sr(2)), 1);
    assert_eq!(
        fake::take_d3d12_calls(),
        [("create", 1), ("wait", 0), ("evaluate", 1)]
    );
}
