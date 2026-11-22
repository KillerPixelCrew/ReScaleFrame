// SPDX-License-Identifier: MIT
//! The native fixture lends the real flush detour and the two scoped calls.
use crate::geometry_registry_abi::{
    RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC, RSF_ANOMALY_TYPE_SKELETON_ST,
};
use crate::native_geometry_abi::{FIRST_PERSON, HARDWARE_SKINNED, SCOPE, UNSET, WORLD};
use std::cell::RefCell;

#[repr(C)]
#[derive(Clone, Copy)]
struct Facts {
    in_visual: u8,
    view: u8,
    domain: u8,
    capture: u8,
    kind: u8,
    verdict: u8,
    visual_type: u8,
}

type Sink = unsafe extern "C" fn(*const Facts);

#[repr(C)]
struct Fixture {
    install: unsafe extern "C" fn(Sink) -> bool,
    observe: unsafe extern "C" fn(bool),
    flush: unsafe extern "C" fn(),
    flush_scoped: unsafe extern "C" fn() -> bool,
    disable: unsafe extern "C" fn() -> bool,
}

thread_local! {
    static CALLS: RefCell<Vec<Option<Facts>>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn receive(facts: *const Facts) {
    // The native recorder lends a facts row for the callback only. Null is the original call.
    CALLS.with(|calls| calls.borrow_mut().push(unsafe { facts.as_ref().copied() }));
}

fn take() -> Vec<Option<Facts>> {
    CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
}

fn check(condition: bool, message: &'static str) -> Result<(), &'static str> {
    if condition { Ok(()) } else { Err(message) }
}

fn exercise(fixture: &Fixture) -> Result<(), &'static str> {
    check(
        unsafe { (fixture.install)(receive) },
        "redirected flush detour installs",
    )?;
    unsafe { (fixture.flush)() };
    check(
        matches!(take().as_slice(), [None]),
        "without a slot only the original runs",
    )?;

    unsafe {
        (fixture.observe)(true);
        (fixture.flush)()
    };
    let calls = take();
    let [None, Some(outside)] = calls.as_slice() else {
        return Err("flush callback follows the original");
    };
    check(
        outside.in_visual == 0
            && outside.view == UNSET
            && outside.domain == UNSET
            && outside.capture == 0,
        "outside every scope",
    )?;

    check(unsafe { (fixture.flush_scoped)() }, "scoped flushes run")?;
    let calls = take();
    let [None, Some(context), None, Some(visual)] = calls.as_slice() else {
        return Err("scoped callbacks follow their original calls");
    };
    check(
        context.in_visual == 0 && context.view == SCOPE && context.domain == WORLD,
        "context supplies the view and domain outside a visual",
    )?;
    check(
        visual.in_visual == 1
            && visual.kind == HARDWARE_SKINNED
            && i32::from(visual.verdict) == RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC
            && i32::from(visual.visual_type) == RSF_ANOMALY_TYPE_SKELETON_ST,
        "visual scope facts",
    )?;
    check(
        visual.view == SCOPE && visual.domain == FIRST_PERSON && visual.capture == 0,
        "first-person scope inherits the view and unidentified draws lack capture",
    )?;

    unsafe {
        (fixture.observe)(false);
        (fixture.flush)()
    };
    check(matches!(take().as_slice(), [None]), "cleared slot")?;
    unsafe { (fixture.observe)(true) };
    check(unsafe { (fixture.disable)() }, "disable detour")?;
    unsafe { (fixture.flush)() };
    check(
        matches!(take().as_slice(), [None]),
        "disabled detour only calls the original",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_hooks_fixture(fixture: &Fixture) -> i32 {
    match exercise(fixture) {
        Ok(()) => {
            println!("anomaly motion hooks: contract passed");
            0
        }
        Err(reason) => {
            eprintln!("FAIL: {reason}");
            1
        }
    }
}
