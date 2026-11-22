// SPDX-License-Identifier: MIT
//! Retirement of a finished token after the imagespace exit cleared the World's full-output flag,
//! with fake leaves standing in for render_hooks.c.
use super::*;
use crate::{RSF_ERROR_NOT_READY, RSF_OK};
use rsf_game_sys::{rsf_game_render_pass, rsf_result};
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};

thread_local! {
    static CALLS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static RESULT: Cell<rsf_result> = const { Cell::new(RSF_OK) };
    static FLAGS: Cell<u32> = const { Cell::new(0) };
    static WORLD_FULL_OUTPUT: Cell<bool> = const { Cell::new(false) };
    static RELEASED: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "C" fn report(_: *const c_char) {}
unsafe extern "C" fn nothing() {}
unsafe extern "C" fn facts(facts: &mut World) {
    facts.full_output_resolved = WORLD_FULL_OUTPUT.get();
}
unsafe extern "C" fn yes() -> bool {
    true
}
unsafe extern "C" fn no() -> bool {
    false
}
unsafe extern "C" fn resolve(_: bool, _: bool) {}
unsafe extern "C" fn refused(_: bool) -> bool {
    false
}
unsafe extern "C" fn adopt(_: *const i32) {}
unsafe extern "C" fn release() {
    RELEASED.set(RELEASED.get() + 1);
}
unsafe extern "C" fn lease(_: u32, _: &mut Lease) {}
unsafe extern "C" fn preflight(
    _: *const rsf_game_render_pass,
    _: *mut c_void,
    _: &mut OutputOwner,
) -> bool {
    false
}
unsafe extern "C" fn reduce(request: &mut Request, result: &mut rsf_result) -> bool {
    CALLS.with_borrow_mut(|calls| calls.push(request.operation));
    request.result_flags = FLAGS.get();
    *result = RESULT.get();
    true
}

// The producer snapshot is no longer current, so a guarded FINISH would refuse at stage 6.
const LEAVES: Leaves = Leaves {
    report,
    refuse: nothing,
    facts,
    same_source: yes,
    resolve,
    read_raster: refused,
    same_resources: refused,
    adopt,
    discard: nothing,
    release,
    lease,
    preflight,
    raster_current: no,
    begin_output: no,
    reduce,
};

fn committed(token: u64, finished: bool) {
    RECOVERY.set(Recovery {
        token,
        committed: true,
        sealed: true,
        finished,
        ..Recovery::EMPTY
    });
    OWNED.fetch_add(1, Ordering::AcqRel);
}

#[test]
fn finished_token_retires_after_imagespace_cleared_full_output() {
    committed(7, true);
    // The imagespace exit cleared the World flag after its own attempt.
    WORLD_FULL_OUTPUT.set(false);
    // Standalone FINISH keeps answering the World flag without another host call.
    assert!(!finish(&LEAVES));
    assert!(CALLS.take().is_empty());

    RESULT.set(RSF_OK);
    FLAGS.set(NEXT_REDUCTION_ALLOWED);
    assert!(retire(&LEAVES));
    assert_eq!(CALLS.take(), [RETIRE]);
    assert_eq!(state().token, 0);
    assert_eq!(RELEASED.get(), 1);
}

#[test]
fn refused_retire_retains_token_and_retries_without_finish() {
    committed(9, true);
    WORLD_FULL_OUTPUT.set(false);
    RESULT.set(RSF_ERROR_NOT_READY);
    FLAGS.set(0);
    assert!(!retire(&LEAVES));
    assert_eq!(state().token, 9);
    assert_eq!(RELEASED.get(), 0);

    // An accepted RETIRE that withholds the next reduction keeps the token too.
    RESULT.set(RSF_OK);
    assert!(!retire(&LEAVES));
    assert_eq!(state().token, 9);

    FLAGS.set(NEXT_REDUCTION_ALLOWED);
    assert!(retire(&LEAVES));
    assert_eq!(CALLS.take(), [RETIRE, RETIRE, RETIRE]);
    assert_eq!(state().token, 0);
    assert_eq!(RELEASED.get(), 1);
}

#[test]
fn unfinished_token_keeps_guarded_finish_admission() {
    committed(11, false);
    WORLD_FULL_OUTPUT.set(true);
    RESULT.set(RSF_OK);
    FLAGS.set(NEXT_REDUCTION_ALLOWED);
    assert!(!retire(&LEAVES));
    assert!(CALLS.take().is_empty());
    assert_eq!(state().token, 11);
    assert_eq!(RELEASED.get(), 0);
    RECOVERY.set(Recovery::EMPTY);
    OWNED.fetch_sub(1, Ordering::AcqRel);
}
