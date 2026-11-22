// SPDX-License-Identifier: MIT
//! FRAME, SUBMISSION and WINDOW order with fake leaves standing in for render_hooks.cpp.
use super::*;
use crate::cpu_abi::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::render_hooks::recovery::fixtures::IDLE;
use rsf_game_sys::{RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_WINDOW};
use std::cell::RefCell;

const CONTEXT: usize = 0x100;
const CHAIN: usize = 0x200;
const WINDOW: usize = 0x300;
const VIEW: usize = 0x400;

struct Fake {
    pass: rsf_game_render_pass,
    frame: rsf_game_render_pass,
    submission: rsf_game_render_pass,
    source: u64,
    inputs: bool,
    window_open: bool,
    frame_open: bool,
    submission_open: bool,
    refused: bool,
    auxiliary_open: bool,
    owner_reads: u32,
    // The native frame counter the second owner read answers.
    changed_frame: u32,
    lease: bool,
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

fn empty() -> rsf_game_render_pass {
    // SAFETY: the SDK pass holds integers, floats and nullable pointers.
    unsafe { std::mem::zeroed() }
}

fn install() {
    FAKE.set(Some(Box::new(Fake {
        pass: empty(),
        frame: empty(),
        submission: empty(),
        source: 2,
        inputs: false,
        window_open: false,
        frame_open: false,
        submission_open: false,
        refused: false,
        auxiliary_open: false,
        owner_reads: 0,
        changed_frame: 5,
        lease: true,
        events: Vec::new(),
    })));
}

unsafe extern "C" fn world(out: &mut World) {
    with(|fake| {
        out.cpu = CpuIdentity {
            session: 1,
            source: fake.source,
            submission: 3,
            began_qpc: 0,
            frequency: 0,
        };
        out.pass = &mut fake.pass;
        out.frame_pass = &mut fake.frame;
        out.submission_pass = &mut fake.submission;
        out.context = CONTEXT as *mut c_void;
        out.inputs = fake.inputs;
        out.published = false;
        out.window_open = fake.window_open;
        out.frame_open = fake.frame_open;
        out.submission_open = fake.submission_open;
        out.refused = fake.refused;
        out.auxiliary_open = fake.auxiliary_open;
    });
}
unsafe extern "C" fn read_owner(out: &mut Owner) -> bool {
    event("read_owner", 0);
    let frame = with(|fake| {
        fake.owner_reads += 1;
        if fake.owner_reads == 1 {
            5
        } else {
            fake.changed_frame
        }
    });
    *out = Owner {
        renderer: 0x10,
        device: 0x20,
        context: CONTEXT,
        chain: CHAIN,
        window: WINDOW,
        graph: 0x30,
        view: VIEW,
        frame,
    };
    true
}
unsafe extern "C" fn lease_owner(owner: &Owner) -> bool {
    event("lease", 0);
    owner.context == CONTEXT && with(|fake| fake.lease)
}
unsafe extern "C" fn adopt_owner() {
    event("adopt", 0);
}
unsafe extern "C" fn discard_owner() {
    event("discard", 0);
}
unsafe extern "C" fn open_frame() {
    event("open_frame", 0);
    with(|fake| fake.frame_open = true);
}
unsafe extern "C" fn open_submission() {
    event("open_submission", 0);
    with(|fake| fake.submission_open = true);
}
unsafe extern "C" fn close_submission_leaf() {
    event("close_submission", 0);
    with(|fake| fake.submission_open = false);
}
unsafe extern "C" fn close_window() {
    event("close_window", 0);
    with(|fake| fake.window_open = false);
}
unsafe extern "C" fn close_frame() {
    event("close_frame", 0);
    with(|fake| fake.frame_open = false);
}
unsafe extern "C" fn drain_auxiliaries() -> bool {
    event("drain", 0);
    true
}
unsafe extern "C" fn finish_resolution() -> bool {
    event("finish_resolution", 0);
    true
}
unsafe extern "C" fn reset() {
    event("reset", 0);
}
unsafe extern "C" fn notify(pass: *const rsf_game_render_pass, context: *mut c_void, begin: u32) {
    // A panic here would abort the test binary, so a wrong Context is recorded instead.
    if context as usize != CONTEXT {
        event("foreign_context", 0);
    }
    let role = unsafe { (*pass).role };
    event(
        match role {
            RSF_GAME_RENDER_FRAME => "frame",
            RSF_GAME_RENDER_SUBMISSION => "submission",
            RSF_GAME_RENDER_WINDOW => "window",
            _ => "other",
        },
        begin,
    );
}
unsafe extern "C" fn diagnostic(_: *const c_char) {
    event("diagnostic", 0);
}
unsafe extern "C" fn report_state(
    stage: u32,
    _: *const rsf_game_render_pass,
    _: *mut c_void,
    _: bool,
    result: rsf_result,
) {
    event("report_state", stage | (u32::from(result == RSF_OK) << 8));
}

const LEAVES: Leaves = Leaves {
    world,
    read_owner,
    lease_owner,
    adopt_owner,
    discard_owner,
    open_frame,
    open_submission,
    close_submission: close_submission_leaf,
    close_window,
    close_frame,
    drain_auxiliaries,
    finish_resolution,
    reset,
    notify,
    diagnostic,
    report_state,
};

unsafe extern "C" fn report(_: *const c_char) {
    event("report", 0);
}
unsafe extern "C" fn refuse() {
    event("refuse", 0);
    with(|fake| fake.refused = true);
}

const RECOVERY: RecoveryLeaves = RecoveryLeaves {
    report,
    refuse,
    ..IDLE
};

fn events() -> Vec<(&'static str, u32)> {
    with(|fake| std::mem::take(&mut fake.events))
}

fn without_reports(events: Vec<(&'static str, u32)>) -> Vec<(&'static str, u32)> {
    // The begin announcement is once per process, so its presence depends on test order.
    events
        .into_iter()
        .filter(|&(name, _)| name != "report")
        .collect()
}

#[test]
fn begin_opens_frame_then_submission_after_a_verified_lease() {
    install();
    rsf_fallout4_render_begin_scope(&LEAVES, &RECOVERY);
    assert_eq!(
        without_reports(events()),
        [
            ("read_owner", 0),
            ("lease", 0),
            ("read_owner", 0),
            ("adopt", 0),
            ("discard", 0),
            ("open_frame", 0),
            ("frame", 1),
            ("open_submission", 0),
            ("submission", 1),
        ]
    );
    with(|fake| {
        assert!(fake.frame_open && fake.submission_open && !fake.refused);
        let frame = &fake.frame;
        assert_eq!(
            frame.struct_size as usize,
            size_of::<rsf_game_render_pass>()
        );
        assert_eq!(frame.role, RSF_GAME_RENDER_FRAME);
        assert_eq!(frame.native_frame, 5);
        assert_eq!((frame.session_id, frame.source_frame_id), (1, 2));
        assert_eq!((frame.submission_id, frame.scope_id), (3, 3));
        assert_eq!(
            (frame.family_key, frame.view_key, frame.history_key),
            (VIEW as u64, VIEW as u64, VIEW as u64)
        );
        assert_eq!(
            (frame.viewport_key, frame.window_key, frame.rhi_viewport_key),
            (WINDOW as u64, WINDOW as u64, WINDOW as u64)
        );
        assert_eq!(frame.pass_key, (image() + FRAME_PASS) as u64);
        assert_eq!(frame.swapchain as usize, CHAIN);
        assert_ne!(frame.flags & RSF_GAME_RENDER_PRIMARY, 0);
        assert_ne!(frame.flags & RSF_GAME_RENDER_NO_FG, 0);
        assert_eq!(fake.submission.role, RSF_GAME_RENDER_SUBMISSION);
        assert_eq!(fake.submission.native_frame, 5);
    });
}

#[test]
fn begin_refuses_an_owner_that_changed_across_the_lease() {
    install();
    with(|fake| fake.changed_frame = 6);
    rsf_fallout4_render_begin_scope(&LEAVES, &RECOVERY);
    assert_eq!(
        events(),
        [
            ("read_owner", 0),
            ("lease", 0),
            ("read_owner", 0),
            ("discard", 0),
            ("refuse", 0),
            ("diagnostic", 0),
        ]
    );
    assert!(with(|fake| !fake.frame_open && !fake.submission_open));
}

#[test]
fn begin_refuses_a_failed_com_lease_without_the_second_read() {
    install();
    with(|fake| fake.lease = false);
    rsf_fallout4_render_begin_scope(&LEAVES, &RECOVERY);
    assert_eq!(
        events(),
        [
            ("read_owner", 0),
            ("lease", 0),
            ("discard", 0),
            ("refuse", 0),
            ("diagnostic", 0),
        ]
    );
}

#[test]
fn present_closes_submission_once_with_refusal() {
    install();
    rsf_fallout4_render_begin_scope(&LEAVES, &RECOVERY);
    let _ = events();
    with(|fake| {
        fake.refused = true;
        fake.submission.flags &= !RSF_GAME_RENDER_NO_FG;
    });
    rsf_fallout4_render_close_submission(&LEAVES);
    rsf_fallout4_render_close_submission(&LEAVES);
    assert_eq!(events(), [("submission", 0), ("close_submission", 0)]);
    with(|fake| {
        assert_ne!(fake.submission.flags & RSF_GAME_RENDER_NO_FG, 0);
        assert!(fake.frame_open);
    });
}

#[test]
fn finish_balances_submission_then_window_then_frame_and_resets() {
    install();
    rsf_fallout4_render_begin_scope(&LEAVES, &RECOVERY);
    let _ = events();
    with(|fake| {
        fake.pass = crate::copy_pass(&fake.frame);
        fake.pass.role = RSF_GAME_RENDER_WINDOW;
        fake.pass.flags &= !RSF_GAME_RENDER_NO_FG;
        fake.frame.flags &= !RSF_GAME_RENDER_NO_FG;
        fake.inputs = true;
        fake.window_open = true;
    });
    rsf_fallout4_render_finish_scope(&LEAVES, &RECOVERY);
    assert_eq!(
        events(),
        [
            ("refuse", 0),
            ("diagnostic", 0),
            ("submission", 0),
            ("close_submission", 0),
            ("window", 0),
            ("close_window", 0),
            ("frame", 0),
            ("close_frame", 0),
            ("drain", 0),
            ("finish_resolution", 0),
            ("report_state", SOURCE_RETIRED | (1 << 8)),
            ("reset", 0),
        ]
    );
    with(|fake| {
        assert_ne!(fake.pass.flags & RSF_GAME_RENDER_NO_FG, 0);
        assert_ne!(fake.frame.flags & RSF_GAME_RENDER_NO_FG, 0);
    });
}

#[test]
fn finish_after_present_keeps_fg_and_retains_open_auxiliaries() {
    install();
    rsf_fallout4_render_begin_scope(&LEAVES, &RECOVERY);
    rsf_fallout4_render_close_submission(&LEAVES);
    let _ = events();
    with(|fake| {
        fake.frame.flags &= !RSF_GAME_RENDER_NO_FG;
        fake.auxiliary_open = true;
    });
    rsf_fallout4_render_finish_scope(&LEAVES, &RECOVERY);
    assert_eq!(
        events(),
        [
            ("frame", 0),
            ("close_frame", 0),
            ("drain", 0),
            ("finish_resolution", 0),
            ("report_state", SOURCE_RETIRED | (1 << 8)),
        ]
    );
    assert_eq!(with(|fake| fake.frame.flags) & RSF_GAME_RENDER_NO_FG, 0);
}

#[test]
fn finish_without_a_source_does_nothing() {
    install();
    with(|fake| {
        fake.source = 0;
        fake.submission_open = true;
    });
    rsf_fallout4_render_finish_scope(&LEAVES, &RECOVERY);
    assert!(events().is_empty());
}
