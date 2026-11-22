// SPDX-License-Identifier: MIT
//! SR and HUDless callback order with fake leaves standing in for render_hooks.cpp.
use super::*;
use crate::render_hooks::recovery::fixtures::IDLE;
use std::cell::RefCell;

const CONTEXT: usize = 0x100;
const CHAIN: usize = 0x200;
const TEXTURE: usize = 0x300;
const WINDOW_KEY: u64 = 0x10;

struct Fake {
    pass: rsf_game_render_pass,
    frame: rsf_game_render_pass,
    basis: rsf_game_render_pass,
    input: rsf_game_render_pass,
    inputs: bool,
    window_open: bool,
    refused: bool,
    config: i32,
    enabled: u32,
    framebuffer_frame: u64,
    framebuffer_acquired: bool,
    events: Vec<(&'static str, u32)>,
}

thread_local! {
    static FAKE: RefCell<Option<Box<Fake>>> = const { RefCell::new(None) };
}

fn with<R>(change: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with_borrow_mut(|fake| change(fake.as_mut().expect("fake installed")))
}

fn event(name: &'static str, value: u32) {
    with(|fake| fake.events.push((name, value)));
}

/// A reduced 960x540 producer pass inside 1920x1080 output that the resolution check accepts.
fn producer() -> rsf_game_render_pass {
    // SAFETY: the SDK pass holds integers, floats and nullable pointers.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.native_frame = 5;
    pass.view_key = 7;
    pass.window_key = WINDOW_KEY;
    pass.render_rect = [0, 0, 960, 540];
    pass.output_rect = [0, 0, 1920, 1080];
    pass.camera.render_width = 960;
    pass.camera.render_height = 540;
    pass.camera.output_width = 1920;
    pass.camera.output_height = 1080;
    pass.camera_valid = 1;
    pass
}

fn install(config: i32, enabled: u32) {
    FAKE.set(Some(Box::new(Fake {
        pass: producer(),
        frame: producer(),
        basis: producer(),
        input: producer(),
        inputs: false,
        window_open: false,
        refused: false,
        config,
        enabled,
        framebuffer_frame: 5,
        framebuffer_acquired: true,
        events: Vec::new(),
    })));
    crate::render_hooks::LIVE.store(true, Ordering::Release);
}

unsafe extern "C" fn world(out: &mut World) {
    with(|fake| {
        out.cpu = CpuIdentity {
            session: 1,
            source: 2,
            submission: 3,
            began_qpc: 0,
            frequency: 0,
        };
        out.pass = &mut fake.pass;
        out.frame_pass = &mut fake.frame;
        out.screen_basis = &mut fake.basis;
        out.context = CONTEXT as *mut c_void;
        out.chain = CHAIN as *mut c_void;
        out.upscale = true;
        out.in_imagespace = true;
        out.inputs = fake.inputs;
        out.window_open = fake.window_open;
        out.frame_open = true;
        out.submission_open = true;
        out.refused = fake.refused;
        out.raster_refused = false;
        out.auxiliary_open = false;
        out.depth_valid = true;
    });
}
unsafe extern "C" fn diagnostic(_: *const c_char) {
    event("diagnostic", 0);
}
unsafe extern "C" fn read_inputs() -> bool {
    event("read_inputs", 0);
    true
}
unsafe extern "C" fn inputs(out: &mut Inputs) {
    with(|fake| {
        out.pass = &fake.input;
        out.context = CONTEXT as *mut c_void;
        out.chain = CHAIN as *mut c_void;
    });
}
unsafe extern "C" fn release_inputs() {
    event("release_inputs", 0);
}
unsafe extern "C" fn yes() -> bool {
    true
}
unsafe extern "C" fn depth_domains(_: *mut rsf_game_render_pass) -> bool {
    event("depth", 0);
    true
}
unsafe extern "C" fn plan(pass: *const rsf_game_render_pass) -> bool {
    let reconfigure = unsafe { (*pass).flags } & RSF_GAME_RENDER_RECONFIGURE != 0;
    event("plan", u32::from(reconfigure));
    true
}
unsafe extern "C" fn render_config(config: &mut rsf_game_render_config) -> i32 {
    event("config", 0);
    with(|fake| {
        config.enabled = fake.enabled;
        config.output_width = 1920;
        config.output_height = 1080;
        fake.config
    })
}
unsafe extern "C" fn retain_inputs(_: *const rsf_game_render_pass) {
    event("retain", 0);
}
unsafe extern "C" fn classify(first: bool, current: &mut Screen, leased: &mut Screen) {
    event("classify", u32::from(first));
    let gameplay = Screen {
        screen: SCREEN_GAMEPLAY,
        facts: 0,
        refusals: 0,
        camera_index: 0,
        camera_state: 0,
    };
    *current = gameplay;
    *leased = gameplay;
}
unsafe extern "C" fn notify(pass: *const rsf_game_render_pass, context: *mut c_void, begin: u32) {
    // A panic here would abort the test binary, so a wrong Context is recorded instead.
    if context as usize != CONTEXT {
        event("foreign_context", 0);
    }
    let role = unsafe { (*pass).role };
    event(
        match role {
            RSF_GAME_RENDER_FG_INPUTS => "fg_inputs",
            RSF_GAME_RENDER_FINAL_SCENE => "final_scene",
            RSF_GAME_RENDER_WINDOW => "window",
            _ => "other",
        },
        begin,
    );
}
unsafe extern "C" fn publish_inputs(scalar: *const rsf_game_render_pass) {
    event("publish_inputs", 0);
    with(|fake| {
        fake.pass = unsafe { *scalar };
        fake.inputs = true;
    });
}
unsafe extern "C" fn begin_output(_: *const rsf_game_render_pass) -> bool {
    event("begin_output", 0);
    true
}
unsafe extern "C" fn report_state(
    stage: u32,
    _: *const rsf_game_render_pass,
    _: *mut c_void,
    _: bool,
    _: i32,
) {
    event("report_state", stage);
}
unsafe extern "C" fn auxiliary_inputs(inputs: &mut AuxiliaryInputs) -> bool {
    inputs.textures = [1, 2, 3, 4, 3].map(|id| id as *mut c_void);
    inputs.raster = [1.25, 2.5, 640.0, 360.0];
    true
}
unsafe extern "C" fn auxiliary_colour(texture: *mut c_void) -> bool {
    !texture.is_null()
}
unsafe extern "C" fn same_texture(first: *mut c_void, second: *mut c_void) -> bool {
    first == second
}
unsafe extern "C" fn reconstruct(
    _: *const rsf_game_render_pass,
    surfaces: *const Surface,
    count: u32,
) -> rsf_result {
    if count != 0 {
        let surfaces = unsafe { std::slice::from_raw_parts(surfaces, count as usize) };
        let plan_matches = count == 4
            && surfaces[0].kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL
            && surfaces[1..]
                .iter()
                .all(|surface| surface.source_rect == [1.25, 2.5, 641.25, 362.5])
            && surfaces[3].flags == RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
        if !plan_matches {
            event("incorrect_auxiliary_plan", count);
        }
    }
    event("reconstruct", 0);
    RSF_OK
}
unsafe extern "C" fn end_output() -> bool {
    event("end_output", 0);
    true
}
unsafe extern "C" fn read_framebuffer(lease: *mut c_void, out: &mut Framebuffer) -> bool {
    event("read_framebuffer", lease as usize as u32);
    out.context = CONTEXT as *mut c_void;
    out.chain = CHAIN as *mut c_void;
    out.texture = TEXTURE as *mut c_void;
    out.native_frame = with(|fake| fake.framebuffer_frame);
    out.window_key = WINDOW_KEY;
    out.extent = [1920, 1080, 1];
    with(|fake| fake.framebuffer_acquired)
}
unsafe extern "C" fn publish_window(lease: *const c_void, window: *const rsf_game_render_pass) {
    event("publish_window", lease as usize as u32);
    with(|fake| {
        fake.pass = unsafe { *window };
        fake.window_open = true;
    });
}

const LEAVES: Leaves = Leaves {
    world,
    diagnostic,
    read_inputs,
    inputs,
    release_inputs,
    same_recovery_resources: yes,
    depth_domains,
    plan,
    render_config,
    retain_inputs,
    classify,
    notify,
    publish_inputs,
    begin_output,
    report_state,
    auxiliary_inputs,
    auxiliary_colour,
    same_texture,
    reconstruct,
    end_output,
    read_framebuffer,
    publish_window,
};

unsafe extern "C" fn refuse() {
    event("refuse", 0);
    with(|fake| fake.refused = true);
}
unsafe extern "C" fn resolve(full_output: bool, published: bool) {
    event(
        "resolve",
        (u32::from(full_output) << 1) | u32::from(published),
    );
}

unsafe extern "C" fn report(text: *const c_char) {
    let text = unsafe { CStr::from_ptr(text) }.to_bytes();
    if text.starts_with(b"Fallout4 HUDless stage=framebuffer-read ") {
        let captured_identity = text
            .windows(b"native=9/5".len())
            .any(|part| part == b"native=9/5");
        event("partial_framebuffer_receipt", u32::from(captured_identity));
    }
}

const RECOVERY: RecoveryLeaves = RecoveryLeaves {
    report,
    refuse,
    resolve,
    ..IDLE
};

fn events() -> Vec<(&'static str, u32)> {
    with(|fake| std::mem::take(&mut fake.events))
}

#[test]
fn published_sr_orders_fg_inputs_before_output_and_evaluation() {
    install(1, 1);
    assert_eq!(emit_sr(&LEAVES, &RECOVERY), SrOutcome::Published);
    assert_eq!(
        events(),
        [
            ("read_inputs", 0),
            ("depth", 0),
            ("plan", 1),
            ("config", 0),
            ("retain", 0),
            ("classify", 1),
            ("fg_inputs", 1),
            ("fg_inputs", 0),
            ("publish_inputs", 0),
            ("begin_output", 0),
            ("report_state", OUTPUT_READY),
            ("reconstruct", 0),
            ("report_state", EVALUATED),
            ("resolve", 3),
            ("diagnostic", 0),
            ("release_inputs", 0),
        ]
    );
    with(|fake| {
        // The retained scalar identity carries no borrowed resource.
        assert_eq!(fake.pass.role, RSF_GAME_RENDER_FG_INPUTS);
        assert_eq!(fake.pass.screen, SCREEN_GAMEPLAY);
        assert_eq!(fake.pass.flags & RSF_GAME_RENDER_NO_FG, 0);
        assert_eq!((fake.pass.session_id, fake.pass.source_frame_id), (1, 2));
        assert_eq!((fake.pass.submission_id, fake.pass.scope_id), (3, 3));
        assert!(fake.pass.swapchain.is_null() && fake.pass.depth.is_null());
    });
}

#[test]
fn sr_off_keeps_fg_inputs_without_output_state() {
    install(1, 0);
    assert_eq!(emit_sr(&LEAVES, &RECOVERY), SrOutcome::Refused);
    let events = events();
    assert_eq!(
        events[events.len() - 2..],
        [("publish_inputs", 0), ("release_inputs", 0)]
    );
    assert!(!events.iter().any(|&(name, _)| name == "begin_output"));
}

#[test]
fn render_config_throw_fails_before_screen_and_callbacks() {
    install(-1, 0);
    assert_eq!(emit_sr(&LEAVES, &RECOVERY), SrOutcome::Failed);
    assert_eq!(
        events(),
        [
            ("read_inputs", 0),
            ("depth", 0),
            ("plan", 1),
            ("config", 0),
            ("release_inputs", 0),
        ]
    );
}

#[test]
fn hudless_opens_window_after_final_scene() {
    install(1, 1);
    assert_eq!(emit_sr(&LEAVES, &RECOVERY), SrOutcome::Published);
    let _ = events();
    emit_hudless(&LEAVES, &RECOVERY, TEXTURE as *mut c_void);
    assert_eq!(
        events(),
        [
            ("read_framebuffer", TEXTURE as u32),
            ("classify", 0),
            ("final_scene", 1),
            ("final_scene", 0),
            ("publish_window", TEXTURE as u32),
            ("window", 1),
        ]
    );
    with(|fake| {
        assert!(fake.window_open);
        assert_eq!(fake.pass.role, RSF_GAME_RENDER_WINDOW);
        assert_eq!(fake.pass.swapchain as usize, CHAIN);
        assert!(fake.pass.scene_surface.is_null());
    });
}

#[test]
fn hudless_refuses_a_foreign_framebuffer_without_callbacks() {
    install(1, 1);
    assert_eq!(emit_sr(&LEAVES, &RECOVERY), SrOutcome::Published);
    let _ = events();
    with(|fake| fake.framebuffer_frame = 6);
    emit_hudless(&LEAVES, &RECOVERY, TEXTURE as *mut c_void);
    assert_eq!(events(), [("read_framebuffer", TEXTURE as u32)]);
    assert!(with(|fake| !fake.window_open));
}

#[test]
fn hudless_reports_partial_facts_from_the_refused_read() {
    install(1, 1);
    assert_eq!(emit_sr(&LEAVES, &RECOVERY), SrOutcome::Published);
    let _ = events();
    with(|fake| {
        fake.framebuffer_frame = 9;
        fake.framebuffer_acquired = false;
    });
    emit_hudless(&LEAVES, &RECOVERY, TEXTURE as *mut c_void);
    assert_eq!(
        events(),
        [
            ("read_framebuffer", TEXTURE as u32),
            ("partial_framebuffer_receipt", 1)
        ]
    );
    assert!(with(|fake| !fake.window_open));
}
