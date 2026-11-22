// SPDX-License-Identifier: MIT
use crate::abi::rsf_fg_retirement;
use crate::session_abi::*;
use rsf_game_sys::{
    RSF_FRAME_SOURCE_RENDERED, RSF_GAME_FRAME_ABI_VERSION, RSF_SCREEN_FLIGHT, rsf_frame_record,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{null_mut, without_provenance_mut};

pub(super) type Raw = *mut c_void;
pub(super) const OK: i32 = 0;
const INVALID: i32 = -1;
pub(super) const INIT_FAILED: i32 = -6;
pub(super) const NOT_SUPPORTED: i32 = -7;
pub(super) const NOT_READY: i32 = -8;
pub(super) const FEATURE_FAILED: i32 = -9;
const SIMULATION_START: u32 = 0;
const RENDER_SUBMIT_START: u32 = 2;
const RENDER_SUBMIT_END: u32 = 3;
const PRESENT_START: u32 = 4;
const PRESENT_END: u32 = 5;
const INPUT_SAMPLE: u32 = 6;

unsafe extern "C" {
    pub(super) fn rsf_fg_session_create(host: *const rsf_fg_host, out: *mut Raw) -> i32;
    pub(super) fn rsf_fg_session_select(
        session: Raw,
        provider: *const rsf_generation_provider,
        setup: *const rsf_generation_setup,
        options: *const rsf_fg_options,
    ) -> i32;
    pub(super) fn rsf_fg_session_destroy(session: Raw) -> i32;
    fn rsf_fg_session_begin(session: Raw, id: u64) -> i32;
    fn rsf_fg_session_begin_rendered(session: Raw, id: u64, source: u64) -> i32;
    fn rsf_fg_session_marker(
        session: Raw,
        id: u64,
        marker: u32,
        timestamp: u64,
        controller: u32,
    ) -> i32;
    fn rsf_fg_session_prepare(session: Raw, list: Raw, frame: *const rsf_fg_frame) -> i32;
    fn rsf_fg_session_after_present(session: Raw, id: u64) -> i32;
    fn rsf_fg_session_abort(session: Raw, id: u64) -> i32;
    fn rsf_fg_session_get_status(session: Raw, out: *mut rsf_fg_session_status) -> i32;
    fn rsf_frame_sequencer_create(capacity: u32, out: *mut Raw) -> i32;
    fn rsf_frame_sequencer_begin(ledger: Raw, id: u64) -> i32;
    fn rsf_frame_sequencer_begin_rendered(ledger: Raw, id: u64, source: u64) -> i32;
    fn rsf_frame_sequencer_marker(ledger: Raw, id: u64, marker: u32, timestamp: u64) -> i32;
    fn rsf_frame_sequencer_finish(ledger: Raw, id: u64) -> i32;
    fn rsf_frame_sequencer_get(ledger: Raw, id: u64, out: *mut rsf_sequence_state) -> i32;
    fn rsf_frame_sequencer_destroy(ledger: Raw);
}

macro_rules! check {
    ($condition:expr) => {
        if !$condition {
            eprintln!(
                "FG contract failed at {}: {}",
                line!(),
                stringify!($condition)
            );
            std::process::exit(1);
        }
    };
}

#[derive(Clone, Copy, Default)]
struct Mocks {
    session: Raw,
    fail_marker: bool,
    reserved_retirement: bool,
    fail_probe: bool,
    fail_chain: bool,
    fail_retire: bool,
    quiescent: bool,
    physical: u32,
    destroyed: u32,
    restored: u32,
    pacing: u32,
    begins: u32,
    markers: u32,
    preparations: u32,
}

thread_local! {
    static MOCKS: Cell<Mocks> = Cell::new(Mocks { pacing: RSF_PACING_REFLEX, ..Mocks::default() });
}

fn mocks() -> Mocks {
    MOCKS.get()
}

fn update(f: impl FnOnce(&mut Mocks)) {
    let mut state = mocks();
    f(&mut state);
    MOCKS.set(state);
}

unsafe extern "C" fn create(
    setup: *const rsf_generation_setup,
    context: *mut Raw,
    chain: *mut Raw,
) -> i32 {
    let state = mocks();
    check!(state.quiescent);
    unsafe {
        context.write(null_mut());
        chain.write(null_mut());
    }
    let physical = !unsafe { (*setup).chain.hwnd }.is_null();
    if !physical && state.fail_probe {
        return NOT_SUPPORTED;
    }
    if physical && state.fail_chain {
        return INIT_FAILED;
    }
    if physical {
        check!(state.physical == 0);
        update(|state| state.physical += 1);
    }
    let raw = Box::into_raw(Box::new(physical)).cast();
    unsafe {
        context.write(raw);
        if physical {
            chain.write(raw);
        }
    }
    OK
}

extern "C" fn configure(_: Raw, _: *const rsf_fg_options) -> i32 {
    check!(mocks().quiescent);
    OK
}
extern "C" fn begin(_: Raw, _: u64) -> i32 {
    update(|s| s.begins += 1);
    OK
}
extern "C" fn marker(_: Raw, _: u32, _: u64, _: u32) -> i32 {
    update(|s| s.markers += 1);
    if mocks().fail_marker {
        FEATURE_FAILED
    } else {
        OK
    }
}
extern "C" fn prepare(_: Raw, _: Raw, _: *const rsf_fg_frame) -> i32 {
    update(|s| s.preparations += 1);
    OK
}
extern "C" fn after(_: Raw) -> i32 {
    OK
}
unsafe extern "C" fn status(_: Raw, out: *mut rsf_fg_status) -> i32 {
    unsafe {
        (*out).supported = 1;
        (*out).max_generated_frames = 1;
        (*out).pacing_owner = mocks().pacing;
    }
    OK
}
unsafe extern "C" fn retire(context: Raw, out: *mut rsf_fg_retirement) -> i32 {
    let state = mocks();
    unsafe {
        (*out).fence = if state.reserved_retirement {
            context
        } else {
            null_mut()
        };
        (*out).value = if state.reserved_retirement {
            u64::MAX
        } else {
            0
        };
    }
    if state.fail_retire { NOT_READY } else { OK }
}
unsafe extern "C" fn destroy(context: Raw) {
    check!(mocks().quiescent);
    let physical = unsafe { Box::from_raw(context.cast::<bool>()) };
    if *physical {
        check!(mocks().physical == 1);
        update(|s| {
            s.physical -= 1;
            s.destroyed += 1;
        });
    }
}
extern "C" fn abort(_: Raw, _: u64) -> i32 {
    OK
}

fn check_transition_is_unpublished() {
    let session = mocks().session;
    if !session.is_null() {
        check!(unsafe { rsf_fg_session_begin(session, 100) } == NOT_READY);
    }
}
extern "C" fn quiesce(_: Raw) -> i32 {
    check_transition_is_unpublished();
    update(|state| state.quiescent = true);
    OK
}
extern "C" fn resume(_: Raw) {
    check!(mocks().quiescent);
    check_transition_is_unpublished();
    update(|state| state.quiescent = false);
}
extern "C" fn release(_: Raw) -> i32 {
    check!(mocks().quiescent);
    OK
}
extern "C" fn adopt(_: Raw, chain: Raw) -> i32 {
    check!(mocks().quiescent && !chain.is_null() && mocks().physical == 1);
    OK
}
extern "C" fn plain(_: Raw) -> i32 {
    check!(mocks().quiescent && mocks().physical == 0);
    update(|s| s.restored += 1);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_fg_session() -> i32 {
    // The public callbacks synchronously observe the same thread's mock provider and host.
    let host = rsf_fg_host {
        struct_size: size_of::<rsf_fg_host>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        user: null_mut(),
        quiesce: Some(quiesce),
        resume: Some(resume),
        release: Some(release),
        adopt: Some(adopt),
        restore_plain: Some(plain),
    };
    let provider = rsf_generation_provider {
        struct_size: size_of::<rsf_generation_provider>() as u32,
        create: Some(create),
        configure: Some(configure),
        begin_frame: Some(begin),
        marker: Some(marker),
        prepare: Some(prepare),
        after_present: Some(after),
        status: Some(status),
        retirement: Some(retire),
        destroy: Some(destroy),
        abort_frame: Some(abort),
        destroy_checked: None,
    };
    let mut live_session = null_mut();
    check!(unsafe { rsf_fg_session_create(&host, &mut live_session) } == OK);
    update(|state| state.session = live_session);
    let mut setup = rsf_generation_setup {
        struct_size: size_of::<rsf_generation_setup>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        ..unsafe { std::mem::zeroed() }
    };
    setup.chain.hwnd = without_provenance_mut(1);
    let mut options = rsf_fg_options {
        struct_size: size_of::<rsf_fg_options>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        mode: RSF_FG_FIXED,
        generated_frames: 1,
        dynamic_target_fps: 0.,
        reflex_mode: RSF_REFLEX_ON,
        frame_limit_us: 0,
    };
    check!(unsafe { rsf_fg_session_select(live_session, &provider, &setup, &options) } == 0);
    update(|state| state.fail_probe = true);
    check!(
        unsafe { rsf_fg_session_select(live_session, &provider, &setup, &options) }
            == NOT_SUPPORTED
    );
    check!(mocks().physical == 1 && mocks().destroyed == 0);
    update(|state| state.fail_probe = false);
    options.mode = RSF_FG_DYNAMIC;
    check!(
        unsafe { rsf_fg_session_select(live_session, &provider, &setup, &options) }
            == NOT_SUPPORTED
    );
    check!(mocks().physical == 1 && mocks().destroyed == 0);
    options.mode = RSF_FG_FIXED;
    check!(unsafe { rsf_fg_session_begin_rendered(live_session, 1, 10) } == NOT_SUPPORTED);
    check!(unsafe { rsf_fg_session_begin(live_session, 1) } == 0);
    check!(
        unsafe { rsf_fg_session_select(live_session, null_mut(), null_mut(), null_mut()) }
            == NOT_READY
    );
    check!(unsafe { rsf_fg_session_marker(live_session, 1, INPUT_SAMPLE, 10, 0) } == 0);
    for i in 0..4_u32 {
        check!(unsafe { rsf_fg_session_marker(live_session, 1, i, 11 + u64::from(i), 0) } == 0);
    }
    let mut record = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        frame_id: 1,
        screen: RSF_SCREEN_FLIGHT,
        ..unsafe { std::mem::zeroed() }
    };
    let frame = rsf_fg_frame {
        struct_size: size_of::<rsf_fg_frame>() as u32,
        record: &raw const record,
        interpolate: 1,
        ..unsafe { std::mem::zeroed() }
    };
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == INVALID);
    record.input_qpc = 10;
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == 0);
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == NOT_READY);
    check!(unsafe { rsf_fg_session_marker(live_session, 1, PRESENT_START, 15, 0) } == 0);
    check!(unsafe { rsf_fg_session_marker(live_session, 1, PRESENT_END, 16, 0) } == 0);
    check!(unsafe { rsf_fg_session_after_present(live_session, 1) } == 0);
    check!(unsafe { rsf_fg_session_begin(live_session, 1) } == NOT_READY);
    check!(unsafe { rsf_fg_session_begin(live_session, 2) } == 0);
    check!(unsafe { rsf_fg_session_marker(live_session, 2, SIMULATION_START, 20, 0) } == INVALID);
    check!(unsafe { rsf_fg_session_abort(live_session, 2) } == 0);
    let (after_abort_markers, after_abort_preparations) = (mocks().markers, mocks().preparations);
    record.frame_id = 2;
    record.input_qpc = 0;
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == NOT_READY);
    check!(unsafe { rsf_fg_session_marker(live_session, 2, SIMULATION_START, 21, 0) } != OK);
    check!(
        mocks().markers == after_abort_markers && mocks().preparations == after_abort_preparations
    );
    record.frame_id = 3;
    record.input_qpc = 30;
    check!(unsafe { rsf_fg_session_begin(live_session, 3) } == OK);
    update(|state| state.fail_marker = true);
    check!(
        unsafe { rsf_fg_session_marker(live_session, 3, INPUT_SAMPLE, 30, 0) } == FEATURE_FAILED
    );
    update(|state| state.fail_marker = false);
    check!(unsafe { rsf_fg_session_marker(live_session, 3, SIMULATION_START, 31, 0) } == NOT_READY);
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == NOT_READY);
    check!(
        unsafe { rsf_fg_session_select(live_session, null_mut(), null_mut(), null_mut()) }
            == NOT_READY
    );
    check!(unsafe { rsf_fg_session_abort(live_session, 3) } == OK);
    record.frame_id = 4;
    record.input_qpc = 40;
    check!(unsafe { rsf_fg_session_begin(live_session, 4) } == OK);
    check!(unsafe { rsf_fg_session_marker(live_session, 4, INPUT_SAMPLE, 40, 0) } == OK);
    for i in 0..4_u32 {
        check!(unsafe { rsf_fg_session_marker(live_session, 4, i, 41 + u64::from(i), 0) } == OK);
    }
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == OK);
    update(|state| state.fail_marker = true);
    check!(
        unsafe { rsf_fg_session_marker(live_session, 4, PRESENT_START, 45, 0) } == FEATURE_FAILED
    );
    update(|state| state.fail_marker = false);
    check!(unsafe { rsf_fg_session_after_present(live_session, 4) } == NOT_READY);
    check!(unsafe { rsf_fg_session_abort(live_session, 4) } == OK);
    record.frame_id = 1;
    record.input_qpc = 10;
    update(|state| state.reserved_retirement = true);
    check!(unsafe { rsf_fg_session_destroy(live_session) } == INVALID);
    check!(mocks().physical == 1 && mocks().destroyed == 0);
    update(|state| state.reserved_retirement = false);
    update(|state| state.fail_retire = true);
    check!(unsafe { rsf_fg_session_destroy(live_session) } == NOT_READY);
    check!(mocks().physical == 1 && mocks().destroyed == 0);
    update(|state| state.fail_retire = false);
    update(|state| state.fail_chain = true);
    check!(
        unsafe { rsf_fg_session_select(live_session, &provider, &setup, &options) } == INIT_FAILED
    );
    check!(mocks().physical == 0 && mocks().destroyed == 1 && mocks().restored == 1);
    update(|state| state.fail_chain = false);
    let mut state = rsf_fg_session_status {
        struct_size: size_of::<rsf_fg_session_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    check!(unsafe { rsf_fg_session_get_status(live_session, &mut state) } == 0);
    check!(state.has_provider == 0 && state.plain_available != 0);
    check!(unsafe { rsf_fg_session_select(live_session, &provider, &setup, &options) } == 0);
    check!(unsafe { rsf_fg_session_destroy(live_session) } == 0);
    update(|state| state.session = null_mut());
    live_session = null_mut();
    check!(mocks().physical == 0 && mocks().destroyed == 2);
    update(|state| state.pacing = RSF_PACING_NONE);
    options.reflex_mode = RSF_REFLEX_OFF;
    check!(unsafe { rsf_fg_session_create(&host, &mut live_session) } == 0);
    update(|state| state.session = live_session);
    check!(unsafe { rsf_fg_session_select(live_session, &provider, &setup, &options) } == 0);
    let (old_begins, old_markers) = (mocks().begins, mocks().markers);
    check!(unsafe { rsf_fg_session_begin_rendered(live_session, 1, 100) } == 0);
    check!(mocks().begins == old_begins);
    check!(unsafe { rsf_fg_session_marker(live_session, 1, RENDER_SUBMIT_START, 101, 0) } == 0);
    check!(unsafe { rsf_fg_session_marker(live_session, 1, RENDER_SUBMIT_END, 102, 0) } == 0);
    record.input_qpc = 0;
    record.source_provenance = RSF_FRAME_SOURCE_RENDERED;
    record.source_qpc = 100;
    record.source_qpc_frequency = 1000;
    check!(unsafe { rsf_fg_session_prepare(live_session, null_mut(), &frame) } == 0);
    check!(unsafe { rsf_fg_session_marker(live_session, 1, PRESENT_START, 103, 0) } == 0);
    check!(unsafe { rsf_fg_session_marker(live_session, 1, PRESENT_END, 104, 0) } == 0);
    check!(mocks().markers == old_markers);
    check!(unsafe { rsf_fg_session_after_present(live_session, 1) } == 0);
    check!(unsafe { rsf_fg_session_destroy(live_session) } == 0);
    update(|state| state.session = null_mut());
    let mut ledger = null_mut();
    check!(unsafe { rsf_frame_sequencer_create(2, &mut ledger) } == 0);
    check!(unsafe { rsf_frame_sequencer_begin(ledger, u64::MAX - 2) } == 0);
    check!(unsafe { rsf_frame_sequencer_begin(ledger, u64::MAX) } == NOT_READY);
    check!(unsafe { rsf_frame_sequencer_finish(ledger, u64::MAX - 2) } == NOT_READY);
    check!(unsafe { rsf_frame_sequencer_begin(ledger, u64::MAX) } == 0);
    check!(unsafe { rsf_frame_sequencer_marker(ledger, u64::MAX, INPUT_SAMPLE, 100) } == 0);
    check!(
        unsafe { rsf_frame_sequencer_marker(ledger, u64::MAX, SIMULATION_START, 99) } == INVALID
    );
    check!(unsafe { rsf_frame_sequencer_finish(ledger, u64::MAX) } == NOT_READY);
    unsafe { rsf_frame_sequencer_destroy(ledger) };
    check!(unsafe { rsf_frame_sequencer_create(2, &mut ledger) } == 0);
    check!(unsafe { rsf_frame_sequencer_begin_rendered(ledger, 1, 0) } == INVALID);
    check!(unsafe { rsf_frame_sequencer_begin_rendered(ledger, 1, 100) } == 0);
    #[repr(C, align(8))]
    struct ShortState([u32; 2]);
    let mut short_state = ShortState([size_of::<u32>() as u32, 0xdeadbeef]);
    check!(
        unsafe { rsf_frame_sequencer_get(ledger, 1, short_state.0.as_mut_ptr().cast()) } == INVALID
    );
    check!(short_state.0[0] == size_of::<u32>() as u32 && short_state.0[1] == 0xdeadbeef);
    let mut rendered = rsf_sequence_state {
        struct_size: size_of::<rsf_sequence_state>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    check!(unsafe { rsf_frame_sequencer_get(ledger, 1, &mut rendered) } == 0);
    check!(
        rendered.source_provenance == RSF_FRAME_SOURCE_RENDERED
            && rendered.source_qpc == 100
            && rendered.timestamps[0] == 0
    );
    check!(unsafe { rsf_frame_sequencer_marker(ledger, 1, RENDER_SUBMIT_START, 101) } == 0);
    check!(unsafe { rsf_frame_sequencer_marker(ledger, 1, RENDER_SUBMIT_END, 102) } == 0);
    check!(unsafe { rsf_frame_sequencer_marker(ledger, 1, PRESENT_START, 103) } == 0);
    check!(unsafe { rsf_frame_sequencer_marker(ledger, 1, PRESENT_END, 104) } == 0);
    check!(unsafe { rsf_frame_sequencer_finish(ledger, 1) } == 0);
    check!(unsafe { rsf_frame_sequencer_begin_rendered(ledger, 2, 200) } == 0);
    check!(unsafe { rsf_frame_sequencer_marker(ledger, 2, INPUT_SAMPLE, 201) } == INVALID);
    check!(unsafe { rsf_frame_sequencer_finish(ledger, 2) } == NOT_READY);
    unsafe { rsf_frame_sequencer_destroy(ledger) };
    println!("FG switching, fallback, retirement, and CPU identity contracts passed");
    0
}
