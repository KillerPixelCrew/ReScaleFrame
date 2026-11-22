// SPDX-License-Identifier: MIT
//! Imagespace, Effects and TAA order with fake leaves standing in for render_hooks.cpp.
use super::*;
use crate::render_hooks::recovery::fixtures::IDLE;
use std::cell::RefCell;

const CONTEXT: usize = 0x100;
const MANAGER: usize = 0x500;

struct Fake {
    pass: rsf_game_render_pass,
    frame: rsf_game_render_pass,
    in_imagespace: bool,
    inputs: bool,
    published: bool,
    frame_open: bool,
    full_output_resolved: bool,
    auxiliary_open: bool,
    raster_valid: bool,
    raster_current: bool,
    same_source: bool,
    end_output: bool,
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
    pass
}

fn install() {
    FAKE.set(Some(Box::new(Fake {
        pass: pass(7),
        frame: pass(5),
        in_imagespace: false,
        inputs: false,
        published: false,
        frame_open: true,
        full_output_resolved: false,
        auxiliary_open: false,
        raster_valid: false,
        raster_current: true,
        same_source: true,
        end_output: true,
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
        out.context = CONTEXT as *mut c_void;
        out.in_imagespace = fake.in_imagespace;
        out.inputs = fake.inputs;
        out.published = fake.published;
        out.frame_open = fake.frame_open;
        out.full_output_resolved = fake.full_output_resolved;
        out.auxiliary_open = fake.auxiliary_open;
        out.raster_valid = fake.raster_valid;
    });
}
unsafe extern "C" fn enter() {
    event("enter", 0);
    with(|fake| fake.in_imagespace = true);
}
unsafe extern "C" fn leave() {
    event("leave", 0);
    with(|fake| {
        fake.in_imagespace = false;
        fake.published = false;
        fake.full_output_resolved = false;
    });
}
unsafe extern "C" fn manager(out: &mut usize) -> bool {
    event("manager", 0);
    *out = MANAGER;
    true
}
unsafe extern "C" fn drain_auxiliaries() -> bool {
    event("drain", 0);
    with(|fake| fake.auxiliary_open = false);
    true
}
unsafe extern "C" fn end_output() -> bool {
    event("end_output", 0);
    with(|fake| fake.end_output)
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
    enter,
    leave,
    manager,
    drain_auxiliaries,
    end_output,
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
unsafe extern "C" fn raster_current() -> bool {
    event("raster_current", 0);
    with(|fake| fake.raster_current)
}

const RECOVERY: RecoveryLeaves = RecoveryLeaves {
    report,
    refuse,
    same_source,
    raster_current,
    ..IDLE
};

fn effects_call(manager: usize, last: u32) -> EffectsCall {
    EffectsCall {
        caller: image() + EFFECTS_RETURN,
        manager,
        first: EFFECTS_FIRST,
        last,
        mode: EFFECTS_MODE,
        flags: EFFECTS_FLAGS,
    }
}

#[test]
fn imagespace_enters_only_from_a_verified_caller_once() {
    install();
    assert!(!rsf_fallout4_render_enter_imagespace(
        &LEAVES,
        &RECOVERY,
        image() + 0xc3_83c6
    ));
    assert_eq!(events(), [("same_source", 0)]);
    for rva in IMAGESPACE_RETURNS {
        with(|fake| fake.in_imagespace = false);
        assert!(rsf_fallout4_render_enter_imagespace(
            &LEAVES,
            &RECOVERY,
            image() + rva
        ));
        assert_eq!(events(), [("same_source", 0), ("enter", 0)]);
    }
    // Already inside: no source check, no second entry.
    assert!(!rsf_fallout4_render_enter_imagespace(
        &LEAVES,
        &RECOVERY,
        image() + IMAGESPACE_RETURNS[0]
    ));
    assert!(events().is_empty());
}

#[test]
fn imagespace_refuses_a_closed_frame_or_a_changed_source() {
    install();
    with(|fake| fake.frame_open = false);
    let caller = image() + IMAGESPACE_RETURNS[1];
    assert!(!rsf_fallout4_render_enter_imagespace(
        &LEAVES, &RECOVERY, caller
    ));
    assert!(events().is_empty());
    with(|fake| {
        fake.frame_open = true;
        fake.same_source = false;
    });
    assert!(!rsf_fallout4_render_enter_imagespace(
        &LEAVES, &RECOVERY, caller
    ));
    assert_eq!(events(), [("same_source", 0)]);
}

#[test]
fn imagespace_exit_retires_drains_then_ends_output_between_receipts() {
    install();
    with(|fake| {
        fake.in_imagespace = true;
        fake.inputs = true;
        fake.published = true;
        fake.full_output_resolved = true;
    });
    rsf_fallout4_render_leave_imagespace(&LEAVES, &RECOVERY);
    assert_eq!(
        events(),
        [
            ("drain", 0),
            ("report_state", UI_RETURN | (1 << 8) | (7 << 16)),
            ("end_output", 0),
            ("report_state", UI_RESTORED | (1 << 8) | (7 << 16)),
            ("leave", 0),
        ]
    );
    with(|fake| assert!(!fake.in_imagespace && !fake.published && !fake.full_output_resolved));
}

#[test]
fn imagespace_exit_refuses_a_failed_output_end_on_the_frame_pass() {
    install();
    with(|fake| {
        fake.in_imagespace = true;
        fake.end_output = false;
    });
    rsf_fallout4_render_leave_imagespace(&LEAVES, &RECOVERY);
    assert_eq!(
        events(),
        [
            ("drain", 0),
            ("report_state", UI_RETURN | (1 << 8) | (5 << 16)),
            ("end_output", 0),
            ("refuse", 0),
            ("report_state", UI_RESTORED | (5 << 16)),
            ("leave", 0),
        ]
    );
}

#[test]
fn effects_emit_sr_only_for_the_native_manager_and_exact_range() {
    install();
    with(|fake| fake.in_imagespace = true);
    let mut emitted = 0;
    let _ = before_effects(&LEAVES, &RECOVERY, &effects_call(MANAGER, 12), || {
        emitted += 1;
        SrOutcome::Published
    });
    assert!(events().is_empty());
    let _ = before_effects(&LEAVES, &RECOVERY, &effects_call(MANAGER + 8, 13), || {
        emitted += 1;
        SrOutcome::Published
    });
    assert_eq!(events(), [("manager", 0)]);
    assert_eq!(emitted, 0);
    let _ = before_effects(&LEAVES, &RECOVERY, &effects_call(MANAGER, 13), || {
        event("emit_sr", 0);
        SrOutcome::Published
    });
    assert_eq!(
        events(),
        [
            ("manager", 0),
            ("same_source", 0),
            ("report_state", PRE_SR | (5 << 16)),
            ("emit_sr", 0),
        ]
    );
    let _ = before_effects(&LEAVES, &RECOVERY, &effects_call(MANAGER, 13), || {
        SrOutcome::Failed
    });
    assert_eq!(
        events(),
        [
            ("manager", 0),
            ("same_source", 0),
            ("report_state", PRE_SR | (5 << 16)),
            ("diagnostic", 0),
        ]
    );
}

#[test]
fn effects_return_reports_then_drains_open_auxiliaries() {
    install();
    with(|fake| {
        fake.in_imagespace = true;
        fake.auxiliary_open = true;
        fake.inputs = true;
        fake.same_source = false;
    });
    assert!(
        before_effects(&LEAVES, &RECOVERY, &effects_call(0, 13), || {
            SrOutcome::Failed
        })
        .is_none()
    );
    assert_eq!(events(), [("manager", 0)]);
    let receipt = before_effects(&LEAVES, &RECOVERY, &effects_call(MANAGER, 13), || {
        SrOutcome::Refused
    })
    .expect("matching Effects cleanup");
    assert_eq!(
        events(),
        [("manager", 0), ("report_state", PRE_SR | (5 << 16))]
    );
    let replaced = CpuIdentity {
        source: receipt.source + 1,
        ..receipt
    };
    rsf_fallout4_render_after_effects(&LEAVES, &RECOVERY, &replaced);
    assert!(events().is_empty());
    rsf_fallout4_render_after_effects(&LEAVES, &RECOVERY, &receipt);
    assert_eq!(
        events(),
        [
            ("report_state", EFFECTS_RETURNED | (1 << 8) | (7 << 16)),
            ("drain", 0),
        ]
    );
}

// The only test that uses the process-wide TAA receipt bits.
#[test]
fn taa_is_excluded_only_while_imagespace_owns_current_full_output() {
    install();
    // Outside imagespace the original answer passes, with one receipt per combination.
    assert!(rsf_fallout4_render_gate_taa(&LEAVES, &RECOVERY, true));
    assert_eq!(events(), [("report", 0)]);
    assert!(rsf_fallout4_render_gate_taa(&LEAVES, &RECOVERY, true));
    assert!(events().is_empty());
    assert!(!rsf_fallout4_render_gate_taa(&LEAVES, &RECOVERY, false));
    assert_eq!(events(), [("report", 0)]);
    // Resolved full output on the current source excludes native TAA.
    with(|fake| {
        fake.in_imagespace = true;
        fake.full_output_resolved = true;
    });
    assert!(!rsf_fallout4_render_gate_taa(&LEAVES, &RECOVERY, true));
    assert_eq!(events(), [("same_source", 0), ("report", 0)]);
    // A valid raster snapshot decides instead of the source.
    with(|fake| {
        fake.raster_valid = true;
        fake.raster_current = false;
    });
    assert!(rsf_fallout4_render_gate_taa(&LEAVES, &RECOVERY, true));
    assert_eq!(events(), [("raster_current", 0), ("report", 0)]);
    with(|fake| fake.raster_current = true);
    assert!(!rsf_fallout4_render_gate_taa(&LEAVES, &RECOVERY, true));
    assert_eq!(events(), [("raster_current", 0)]);
}
