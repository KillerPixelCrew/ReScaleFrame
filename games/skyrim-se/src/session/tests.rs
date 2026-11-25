// SPDX-License-Identifier: MIT
use super::*;
use crate::observers::fake::DISABLED;
use std::cell::{Cell, RefCell};

thread_local! {
    static OPERATIONS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static OBSERVER_BUSY: Cell<bool> = const { Cell::new(false) };
    static ROLLBACK: Cell<bool> = const { Cell::new(true) };
    static ACTIVE_RESULT: Cell<i32> = const { Cell::new(OK) };
}

/// Recording fakes of the subsystems: admission and every other stage succeed; the CPU start
/// refuses.
pub(super) mod subsystems {
    use super::*;

    fn record(operation: &'static str, value: bool) -> bool {
        OPERATIONS.with_borrow_mut(|operations| operations.push(operation));
        value
    }

    pub(in crate::session) fn admit_observers() -> i32 {
        i32::from(record("admit_observers", true))
    }
    pub(in crate::session) fn discovery_refusal() -> *const c_char {
        ptr::null()
    }
    pub(in crate::session) fn executable_sha256() -> &'static CStr {
        c""
    }
    pub(in crate::session) fn observers_busy() -> bool {
        record("observers_busy", OBSERVER_BUSY.get())
    }
    pub(in crate::session) fn reset_observations() {
        record("reset_observations", true);
    }
    pub(in crate::session) fn cpu_prepare() -> bool {
        record("cpu_prepare", true)
    }
    pub(in crate::session) fn cpu_bind(_: &rsf_game_host_services) -> bool {
        record("cpu_bind", true)
    }
    pub(in crate::session) fn cpu_start() -> bool {
        record("cpu_start", false)
    }
    pub(in crate::session) fn cpu_quiesce() {
        record("cpu_quiesce", true);
    }
    pub(in crate::session) fn cpu_disable() -> bool {
        record("cpu_disable", true)
    }
    pub(in crate::session) fn cpu_stop() -> bool {
        record("cpu_stop", true)
    }
    pub(in crate::session) fn cpu_busy() -> bool {
        record("cpu_busy", false)
    }
    pub(in crate::session) fn creation_start() -> bool {
        record("creation_start", true)
    }
    pub(in crate::session) fn creation_stop() -> bool {
        record("creation_stop", true)
    }
    pub(in crate::session) fn sampling_start(_: &rsf_game_host_services) -> bool {
        record("sampling_start", true)
    }
    pub(in crate::session) fn sampling_quiesce() {
        record("sampling_quiesce", true);
    }
    pub(in crate::session) fn sampling_stop() -> bool {
        record("sampling_stop", true)
    }
    pub(in crate::session) fn sampling_busy() -> bool {
        record("sampling_busy", false)
    }
    pub(in crate::session) fn presentation_start(_: &rsf_game_host_services) -> bool {
        record("presentation_start", true)
    }
    pub(in crate::session) fn presentation_quiesce() {
        record("presentation_quiesce", true);
    }
    pub(in crate::session) fn presentation_stop() -> bool {
        record("presentation_stop", ROLLBACK.get())
    }
    pub(in crate::session) fn presentation_busy() -> bool {
        record("presentation_busy", false)
    }
    pub(in crate::session) fn resolution_owned() -> bool {
        record("resolution_owned", false)
    }
    pub(in crate::session) fn auxiliaries_pending() -> bool {
        record("auxiliaries_pending", false)
    }
    pub(in crate::session) fn checkpoint(_: &mut *const c_char, _: bool) -> bool {
        true
    }
    pub(in crate::session) fn log(_: &rsf_game_host_services, _: *const c_char) -> bool {
        true
    }
    pub(in crate::session) fn host_create(
        _: usize,
        _: &HostSetup,
        _: &mut rsf_game_host_services,
        _: &mut i32,
    ) -> bool {
        true
    }
    pub(in crate::session) fn host_set_active(
        _: usize,
        _: *mut c_void,
        _: u32,
        result: &mut i32,
    ) -> bool {
        *result = ACTIVE_RESULT.get();
        true
    }
    pub(in crate::session) extern "C" fn accept_window(_: *mut c_void, _: *mut c_void) -> i32 {
        0
    }
}

fn prepared() -> Session {
    let mut session = Session::new();
    assert!(session.observers.prepare());
    session.prepared = true;
    OPERATIONS.take();
    DISABLED.set(0);
    session
}

/// Native creation goes live before every other hook, and retires last.
#[test]
fn a_refused_stage_and_its_predecessors_retire() {
    let mut session = prepared();
    assert!(matches!(session.start(), Ok(NATIVE_REFUSED)));
    assert_eq!(
        OPERATIONS.take(),
        [
            "admit_observers",
            "creation_start",
            "sampling_start",
            "presentation_start",
            "cpu_start",
            "cpu_disable",
            "presentation_stop",
            "sampling_stop",
            "creation_stop",
        ]
    );
    assert_eq!(DISABLED.get(), 5);
    assert!(!session.enabled);
    assert_eq!(session.reason, Stage::Cpu.refusal().as_ptr());
}

#[test]
fn quiesce_withdraws_a_start_that_published_while_it_waited() {
    let mut session = prepared();
    // The public entry withdrew before acquiring the session lock. An in-flight start
    // then published while quiesce was waiting for the lock.
    ACCEPTING.store(true, Ordering::Release);
    OBSERVER_BUSY.set(true);
    assert!(matches!(session.quiesce(), Ok(BUSY)));
    OBSERVER_BUSY.set(false);
    assert!(!ACCEPTING.load(Ordering::Acquire));
    assert!(matches!(session.start(), Ok(BUSY)));
    assert!(matches!(session.quiesce(), Ok(OK)));
}

#[test]
fn a_refused_rollback_leaves_stop_owed() {
    ROLLBACK.set(false);
    let mut session = prepared();
    assert!(matches!(session.start(), Ok(BUSY)));
    assert!(session.retirement_pending);
    OPERATIONS.take();
    assert!(matches!(session.start(), Ok(BUSY)));
    assert!(OPERATIONS.take().is_empty());
    assert!(matches!(session.quiesce(), Ok(BUSY)));
    ROLLBACK.set(true);
    assert!(matches!(session.quiesce(), Ok(OK)));
    assert_eq!(session.rollback_stages, 0);
    assert!(matches!(session.start(), Ok(BUSY)));
}

#[test]
fn runtime_deactivation_refusal_survives_the_native_call() {
    let mut session = prepared();
    ACTIVE_RESULT.set(INVALID_ARGUMENT);
    session.activate = 1;
    session.bound_services.user = ptr::dangling_mut();
    assert!(matches!(session.suspend_host(), Ok(INVALID_ARGUMENT)));
    ACTIVE_RESULT.set(OK);
}
