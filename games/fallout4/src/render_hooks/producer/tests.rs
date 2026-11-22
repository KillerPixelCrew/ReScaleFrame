// SPDX-License-Identifier: MIT
//! Viewport, DR and prepass order with fake leaves standing in for render_hooks.c.
use super::*;
use crate::render_hooks::recovery::fixtures::IDLE;
use std::cell::RefCell;
use std::sync::atomic::Ordering;

const CONTEXT: usize = 0x100;
const CHAIN: usize = 0x200;
const VIEW: usize = 0x300;
const WINDOW: usize = 0x400;
const SOURCE: u64 = 2;

struct Fake {
    pass: rsf_game_render_pass,
    frame: rsf_game_render_pass,
    inputs: bool,
    frame_open: bool,
    submission_open: bool,
    depth_interrupted: bool,
    same_source: bool,
    screen_refusals: u32,
    owner: Option<Owner>,
    configure: bool,
    native_dr: bool,
    capture_depth: bool,
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

fn events() -> Vec<(&'static str, u32)> {
    with(|fake| std::mem::take(&mut fake.events))
}

fn pass(native_frame: u64) -> rsf_game_render_pass {
    // SAFETY: the SDK pass holds integers, floats and nullable pointers.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.native_frame = native_frame;
    pass.view_key = VIEW as u64;
    pass.window_key = WINDOW as u64;
    pass
}

fn owner() -> Owner {
    Owner {
        context: CONTEXT,
        chain: CHAIN,
        window: WINDOW,
        view: VIEW,
        frame: 5,
        ..Owner::default()
    }
}

fn install() {
    FAKE.set(Some(Box::new(Fake {
        pass: pass(7),
        frame: pass(5),
        inputs: false,
        frame_open: true,
        submission_open: true,
        depth_interrupted: false,
        same_source: true,
        screen_refusals: WORLD_LEASE_INVALID,
        owner: Some(owner()),
        configure: true,
        native_dr: true,
        capture_depth: true,
        events: Vec::new(),
    })));
    crate::render_hooks::LIVE.store(true, Ordering::Release);
}

unsafe extern "C" fn world(out: &mut World) {
    with(|fake| {
        out.cpu = CpuIdentity {
            session: 1,
            source: SOURCE,
            submission: 0,
            began_qpc: 0,
            frequency: 0,
        };
        out.pass = &mut fake.pass;
        out.frame_pass = &mut fake.frame;
        out.context = CONTEXT as *mut c_void;
        out.chain = CHAIN as *mut c_void;
        out.inputs = fake.inputs;
        out.published = false;
        out.frame_open = fake.frame_open;
        out.submission_open = fake.submission_open;
        out.depth_interrupted = fake.depth_interrupted;
    });
}
unsafe extern "C" fn invalidate(raster: bool) {
    event("invalidate", u32::from(raster));
}
unsafe extern "C" fn interrupt_depth(interrupted: bool) {
    event("interrupt", u32::from(interrupted));
    with(|fake| fake.depth_interrupted = interrupted);
}
unsafe extern "C" fn refuse_raster() {
    event("refuse_raster", 0);
}
unsafe extern "C" fn native_dr() -> bool {
    event("native_dr", 0);
    with(|fake| fake.native_dr)
}
unsafe extern "C" fn screen_refusals() -> u32 {
    event("screen", 0);
    with(|fake| fake.screen_refusals)
}
unsafe extern "C" fn read_owner(out: &mut Owner) -> bool {
    event("owner", 0);
    let Some(owner) = with(|fake| fake.owner) else {
        return false;
    };
    *out = owner;
    true
}
unsafe extern "C" fn configure() -> bool {
    event("configure", 0);
    with(|fake| fake.configure)
}
unsafe extern "C" fn capture_raster(identity: &CpuIdentity) -> bool {
    event("capture_raster", identity.source as u32);
    false
}
unsafe extern "C" fn capture_depth(identity: &CpuIdentity, _: &mut Snapshot) -> bool {
    event("capture_depth", identity.source as u32);
    with(|fake| fake.capture_depth)
}
unsafe extern "C" fn adopt_depth(_: &Snapshot) {
    event("adopt_depth", 0);
}
unsafe extern "C" fn report_state(
    stage: u32,
    pass: *const rsf_game_render_pass,
    context: *mut c_void,
    _: bool,
    result: rsf_result,
) {
    // A panic here would abort the test binary, so a wrong Context is recorded instead.
    if context as usize != CONTEXT {
        event("foreign_context", 0);
    }
    // Stage, then whether the result is RSF_OK, then the native frame of the reported pass.
    let native = unsafe { (*pass).native_frame } as u32;
    event(
        "report_state",
        stage | (u32::from(result == RSF_OK) << 8) | (native << 16),
    );
}
unsafe extern "C" fn diagnostic(_: *const c_char) {
    event("diagnostic", 0);
}

const LEAVES: Leaves = Leaves {
    world,
    invalidate,
    interrupt_depth,
    refuse_raster,
    native_dr,
    screen_refusals,
    read_owner,
    configure,
    capture_raster,
    capture_depth,
    adopt_depth,
    report_state,
    diagnostic,
};

unsafe extern "C" fn report(_: *const c_char) {
    event("report", 0);
}
unsafe extern "C" fn refuse() {
    event("refuse", 0);
}
unsafe extern "C" fn same_source() -> bool {
    event("same_source", 0);
    with(|fake| fake.same_source)
}

const RECOVERY: RecoveryLeaves = RecoveryLeaves {
    report,
    refuse,
    same_source,
    ..IDLE
};

fn viewport(caller: usize, manager: usize, write: bool) {
    after_viewport(
        &LEAVES,
        &RECOVERY,
        caller,
        manager,
        true,
        |enabled| {
            event("viewport_write", u32::from(enabled));
            write
        },
        || event("emit_hudless", 0),
    );
}

fn dynamic_resolution(caller: usize, manager: usize, busy: bool, refused: bool) {
    after_dynamic_resolution(
        &LEAVES,
        &RECOVERY,
        caller,
        manager,
        || busy,
        || {
            event("resolution_refused", 0);
            refused
        },
    );
}

fn prepass(call: &PrepassCall, current: bool) {
    after_prepass(&LEAVES, &RECOVERY, call, |identity| {
        event("source_current", identity.source as u32);
        current
    });
}

#[test]
fn viewport_writes_the_owned_selector_only_on_the_native_targets() {
    install();
    let targets = image() + TARGETS;
    viewport(image() + HUDLESS_RETURN, targets + 8, true);
    assert!(events().is_empty());
    viewport(image() + HUDLESS_RETURN - 1, targets, true);
    assert_eq!(events(), [("viewport_write", 1)]);
    viewport(image() + HUDLESS_RETURN - 1, targets, false);
    assert_eq!(
        events(),
        [("viewport_write", 1), ("refuse", 0), ("diagnostic", 0)]
    );
}

#[test]
fn hudless_selection_reports_on_the_state_pass_then_emits() {
    install();
    let targets = image() + TARGETS;
    viewport(image() + HUDLESS_RETURN, targets, true);
    assert_eq!(
        events(),
        [
            ("viewport_write", 1),
            ("report_state", HUDLESS | (1 << 8) | (5 << 16)),
            ("emit_hudless", 0),
        ]
    );
    // A failed selector write still refuses first; inputs move the receipt to the input pass.
    with(|fake| fake.inputs = true);
    viewport(image() + HUDLESS_RETURN, targets, false);
    assert_eq!(
        events(),
        [
            ("viewport_write", 1),
            ("refuse", 0),
            ("diagnostic", 0),
            ("report_state", HUDLESS | (1 << 8) | (7 << 16)),
            ("emit_hudless", 0),
        ]
    );
}

#[test]
fn dynamic_resolution_drops_depth_only_inside_an_open_frame() {
    install();
    before_dynamic_resolution(&LEAVES);
    assert_eq!(events(), [("invalidate", 0)]);
    with(|fake| fake.frame_open = false);
    before_dynamic_resolution(&LEAVES);
    assert!(events().is_empty());
}

#[test]
fn overlapping_writer_refuses_transfers_and_skips_configuration() {
    install();
    let caller = image() + DYNAMIC_RESOLUTION_RETURN;
    let targets = image() + TARGETS;
    with(|fake| fake.native_dr = false);
    dynamic_resolution(caller, targets, true, false);
    assert_eq!(
        events(),
        [
            ("refuse_raster", 0),
            ("native_dr", 0),
            ("diagnostic", 0),
            ("refuse_raster", 0),
        ]
    );
    // Off the targets nothing overlaps, but pending ownership still refuses an open FRAME.
    dynamic_resolution(caller, targets + 8, true, false);
    assert_eq!(events(), [("refuse_raster", 0)]);
    with(|fake| fake.frame_open = false);
    dynamic_resolution(caller, targets + 8, true, false);
    assert!(events().is_empty());
}

#[test]
fn selected_writer_configures_on_the_leased_owner() {
    install();
    let caller = image() + DYNAMIC_RESOLUTION_RETURN;
    let targets = image() + TARGETS;
    dynamic_resolution(caller, targets, false, false);
    assert_eq!(
        events(),
        [
            ("same_source", 0),
            ("screen", 0),
            ("owner", 0),
            ("configure", 0)
        ]
    );
    // A failed configuration refuses the raster only with pending ownership or a refused change.
    with(|fake| fake.configure = false);
    dynamic_resolution(caller, targets, false, false);
    assert_eq!(
        events(),
        [
            ("same_source", 0),
            ("screen", 0),
            ("owner", 0),
            ("configure", 0),
            ("resolution_refused", 0),
        ]
    );
    dynamic_resolution(caller, targets, false, true);
    assert_eq!(
        events(),
        [
            ("same_source", 0),
            ("screen", 0),
            ("owner", 0),
            ("configure", 0),
            ("resolution_refused", 0),
            ("refuse_raster", 0),
        ]
    );
}

#[test]
fn selected_writer_refuses_configuration_without_the_exact_screen_or_owner() {
    install();
    let caller = image() + DYNAMIC_RESOLUTION_RETURN;
    let targets = image() + TARGETS;
    with(|fake| fake.screen_refusals = WORLD_LEASE_INVALID | 1);
    dynamic_resolution(caller, targets, false, false);
    assert_eq!(events(), [("same_source", 0), ("screen", 0)]);
    with(|fake| {
        fake.screen_refusals = WORLD_LEASE_INVALID;
        fake.owner = Some(Owner {
            frame: 6,
            ..owner()
        });
    });
    dynamic_resolution(caller, targets, false, false);
    assert_eq!(events(), [("same_source", 0), ("screen", 0), ("owner", 0)]);
    with(|fake| fake.owner = None);
    dynamic_resolution(caller, targets, false, false);
    assert_eq!(events(), [("same_source", 0), ("screen", 0), ("owner", 0)]);
    // After inputs the call is not selected and no source check runs.
    with(|fake| fake.inputs = true);
    dynamic_resolution(caller, targets, false, false);
    assert!(events().is_empty());
}

#[test]
fn prepass_selects_the_outermost_verified_call_and_invalidates_every_frame_call() {
    install();
    let caller = image() + PREPASS_RETURN;
    with(|fake| fake.depth_interrupted = true);
    let call = before_prepass(&LEAVES, &RECOVERY, caller, 1);
    assert!(call.selected && call.identity.source == SOURCE);
    assert_eq!(events(), [("invalidate", 1), ("interrupt", 0)]);
    // A nested call interrupts the selected one.
    let nested = before_prepass(&LEAVES, &RECOVERY, caller, 2);
    assert!(!nested.selected);
    assert_eq!(events(), [("invalidate", 1), ("interrupt", 1)]);
    // A foreign call after inputs invalidates and refuses.
    with(|fake| fake.inputs = true);
    let foreign = before_prepass(&LEAVES, &RECOVERY, caller - 1, 1);
    assert!(!foreign.selected);
    assert_eq!(events(), [("invalidate", 1), ("refuse", 0)]);
    with(|fake| {
        fake.inputs = false;
        fake.frame_open = false;
    });
    assert!(!before_prepass(&LEAVES, &RECOVERY, caller, 1).selected);
    assert!(events().is_empty());
}

#[test]
fn prepass_capture_adopts_only_a_same_source_snapshot() {
    install();
    let caller = image() + PREPASS_RETURN;
    let call = before_prepass(&LEAVES, &RECOVERY, caller, 1);
    events();
    prepass(&call, true);
    assert_eq!(
        events(),
        [
            ("capture_raster", SOURCE as u32),
            ("capture_depth", SOURCE as u32),
            ("source_current", SOURCE as u32),
            ("same_source", 0),
            ("adopt_depth", 0),
        ]
    );
    prepass(&call, false);
    assert_eq!(
        events(),
        [
            ("capture_raster", SOURCE as u32),
            ("capture_depth", SOURCE as u32),
            ("source_current", SOURCE as u32),
            ("refuse", 0),
            ("diagnostic", 0),
        ]
    );
    with(|fake| fake.capture_depth = false);
    prepass(&call, true);
    assert_eq!(
        events(),
        [
            ("capture_raster", SOURCE as u32),
            ("capture_depth", SOURCE as u32),
            ("refuse", 0),
            ("diagnostic", 0),
        ]
    );
}

#[test]
fn prepass_capture_skips_an_interrupted_or_unselected_call() {
    install();
    let caller = image() + PREPASS_RETURN;
    let call = before_prepass(&LEAVES, &RECOVERY, caller, 1);
    let _ = before_prepass(&LEAVES, &RECOVERY, caller, 2);
    events();
    prepass(&call, true);
    assert!(events().is_empty());
    with(|fake| fake.depth_interrupted = false);
    let unselected = before_prepass(&LEAVES, &RECOVERY, caller, 2);
    with(|fake| fake.depth_interrupted = false);
    events();
    prepass(&unselected, true);
    assert!(events().is_empty());
}
