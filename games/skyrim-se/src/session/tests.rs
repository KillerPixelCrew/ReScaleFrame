// SPDX-License-Identifier: MIT
use super::*;
use std::cell::{Cell, RefCell};

thread_local! {
    static OPERATIONS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static CPU_START_THROWS: Cell<bool> = const { Cell::new(false) };
    static ROLLBACK: Cell<i32> = const { Cell::new(1) };
    static ACTIVE_RESULT: Cell<i32> = const { Cell::new(OK) };
    static DISABLED: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "C" fn subsystem(
    operation: u32,
    _: *const rsf_game_host_services,
    value: *mut i32,
) -> bool {
    OPERATIONS.with_borrow_mut(|operations| operations.push(operation));
    // Admission and every other stage succeed; the CPU start refuses or throws.
    if operation == PRESENTATION_STOP {
        let outcome = ROLLBACK.get();
        if outcome < 0 {
            return false;
        }
        unsafe { value.write(outcome) };
        return true;
    }
    unsafe { value.write(i32::from(operation != CPU_START)) };
    operation != CPU_START || !CPU_START_THROWS.get()
}

unsafe extern "C" fn checkpoint(_: *mut *const c_char, _: bool) -> bool {
    true
}

unsafe extern "C" fn compatibility_status() -> *const c_char {
    ptr::null()
}

unsafe extern "C" fn log(_: *const rsf_game_host_services, _: *const c_char) -> bool {
    true
}

unsafe extern "C" fn module_path(_: usize, _: *mut u16, _: u32) -> u32 {
    0
}

unsafe extern "C" fn host_create(
    _: usize,
    _: *const HostSetup,
    _: *mut rsf_game_host_services,
    _: *mut i32,
) -> bool {
    true
}

unsafe extern "C" fn host_set_active(_: usize, _: *mut c_void, _: u32, result: *mut i32) -> bool {
    unsafe { result.write(ACTIVE_RESULT.get()) };
    true
}

unsafe extern "C" fn accept_window(_: *mut c_void, _: *mut c_void) -> c_int {
    0
}

unsafe extern "C" fn yes() -> bool {
    true
}

unsafe extern "C" fn hook_yes(_: u32) -> bool {
    true
}

unsafe extern "C" fn enable(_: u32) -> i32 {
    0
}

unsafe extern "C" fn disable(_: u32) -> bool {
    DISABLED.set(DISABLED.get() + 1);
    true
}

unsafe extern "C" fn observer_log(_: *const rsf_game_host_services, _: *const c_char) {}

const OBSERVERS: observers::Leaves = observers::Leaves {
    initialize: yes,
    create: hook_yes,
    remove: hook_yes,
    enable,
    disable,
    pin: yes,
    log: observer_log,
};

fn leaves() -> Leaves {
    Leaves {
        _struct_size: size_of::<Leaves>() as u32,
        _version: LEAVES_VERSION,
        // Start never reads the creation platform; the CPU stage fails before creation.
        platform: ptr::null(),
        observers: &OBSERVERS,
        subsystem,
        checkpoint,
        compatibility_status,
        log,
        module_path,
        host_create,
        host_set_active,
        accept_window,
    }
}

fn prepared() -> Session {
    let mut session = Session::new();
    assert!(session.observers.prepare(&OBSERVERS));
    session.prepared = true;
    OPERATIONS.take();
    DISABLED.set(0);
    session
}

#[test]
fn a_refused_stage_and_its_predecessors_retire() {
    CPU_START_THROWS.set(false);
    let leaves = leaves();
    let mut session = prepared();
    assert!(matches!(session.start(&leaves), Ok(NATIVE_REFUSED)));
    assert_eq!(
        OPERATIONS.take(),
        [
            ADMIT_OBSERVERS,
            SAMPLING_START,
            PRESENTATION_START,
            CPU_START,
            CPU_DISABLE,
            PRESENTATION_STOP,
            SAMPLING_STOP,
        ]
    );
    assert_eq!(DISABLED.get(), 5);
    assert!(!session.enabled);
    assert_eq!(session.reason, Stage::Cpu.refusal().as_ptr());
}

#[test]
fn a_throwing_stage_is_retired_with_every_earlier_stage() {
    CPU_START_THROWS.set(true);
    let leaves = leaves();
    let mut session = prepared();
    assert!(matches!(session.start(&leaves), Ok(NOT_READY)));
    assert_eq!(
        OPERATIONS.take(),
        [
            ADMIT_OBSERVERS,
            SAMPLING_START,
            PRESENTATION_START,
            CPU_START,
            CPU_DISABLE,
            PRESENTATION_STOP,
            SAMPLING_STOP,
        ]
    );
    assert_eq!(DISABLED.get(), 5);
    assert!(!session.enabled);
    // A throw keeps the reason set before admission, as the former catch did.
    assert_eq!(session.reason, START_IDENTITY.as_ptr());
}

#[test]
fn quiesce_withdraws_a_start_that_published_while_it_waited() {
    let leaves = leaves();
    let mut session = prepared();
    // The public entry withdrew before acquiring the session lock. An in-flight start
    // then published while quiesce was waiting for the lock.
    ACCEPTING.store(true, Ordering::Release);
    assert!(matches!(session.quiesce(&leaves), Ok(BUSY)));
    assert!(!ACCEPTING.load(Ordering::Acquire));
}

#[test]
fn refused_and_throwing_rollback_both_leave_stop_owed() {
    for outcome in [0, -1] {
        ROLLBACK.set(outcome);
        let leaves = leaves();
        let mut session = prepared();
        assert!(matches!(session.start(&leaves), Ok(BUSY)));
        assert!(session.retirement_pending);
        OPERATIONS.take();
        assert!(matches!(session.start(&leaves), Ok(BUSY)));
        assert!(OPERATIONS.take().is_empty());
    }
    ROLLBACK.set(1);
}

#[test]
fn runtime_deactivation_refusal_survives_the_native_call() {
    let leaves = leaves();
    let mut session = prepared();
    ACTIVE_RESULT.set(INVALID_ARGUMENT);
    session.activate = 1;
    session.bound_services.user = ptr::dangling_mut();
    assert!(matches!(
        session.suspend_host(&leaves),
        Ok(INVALID_ARGUMENT)
    ));
    ACTIVE_RESULT.set(OK);
}
