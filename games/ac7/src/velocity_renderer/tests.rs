// SPDX-License-Identifier: MIT
use super::*;
use std::cell::RefCell;

thread_local! {
    // What happened on this test's thread, in order.
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn event(text: String) {
    EVENTS.with(|events| events.borrow_mut().push(text));
}

fn events() -> Vec<String> {
    EVENTS.with(|events| events.take())
}

fn device(identity: usize) -> *mut c_void {
    std::ptr::without_provenance_mut(identity)
}

/// A renderer standing in for the native one: a boxed name.
fn make(name: u32) -> *mut c_void {
    event(format!("create {name}"));
    Box::into_raw(Box::new(name)).cast()
}

fn name(renderer: *mut c_void) -> u32 {
    // SAFETY: Tests only pass renderers `make` returned and nothing destroyed yet.
    unsafe { *renderer.cast::<u32>() }
}

unsafe fn destroy(renderer: *mut c_void) {
    // SAFETY: `make` boxed every renderer this owner destroys.
    let name = *unsafe { Box::from_raw(renderer.cast::<u32>()) };
    event(format!("destroy {name}"));
}

fn admit_as(state: &Mutex<Lifetime>, identity: usize, renderer: u32) -> *mut c_void {
    admit(
        state,
        device(identity),
        || make(renderer),
        destroy,
        || event("published".into()),
    )
}

fn release_all(state: &Mutex<Lifetime>) {
    release(state, || event("released".into()));
}

#[test]
fn null_device_creates_nothing() {
    let state = Mutex::default();
    assert!(admit_as(&state, 0, 1).is_null());
    assert!(events().is_empty());
    assert!(published(&state).is_null());
}

#[test]
fn first_draw_publishes_and_the_same_device_reuses() {
    let state = Mutex::default();
    let first = admit_as(&state, 0x10, 1);
    assert_eq!(name(first), 1);
    assert_eq!(events(), ["create 1", "published"]);
    assert_eq!(published(&state), first);

    assert_eq!(admit_as(&state, 0x10, 2), first);
    assert!(events().is_empty());
    release_all(&state);
}

#[test]
fn another_device_replaces_and_destroys_before_the_resets() {
    let state = Mutex::default();
    admit_as(&state, 0x10, 1);
    events();
    let second = admit_as(&state, 0x20, 2);
    assert_eq!(name(second), 2);
    assert_eq!(events(), ["create 2", "destroy 1", "published"]);
    assert_eq!(published(&state), second);
    assert!(published_for(&state, device(0x10)).is_null());
    assert_eq!(published_for(&state, device(0x20)), second);
    release_all(&state);
}

#[test]
fn failed_creation_keeps_the_published_renderer() {
    let state = Mutex::default();
    let first = admit_as(&state, 0x10, 1);
    events();
    let refused = admit(
        &state,
        device(0x20),
        || {
            event("create failed".into());
            null_mut()
        },
        destroy,
        || event("published".into()),
    );
    assert!(refused.is_null());
    assert_eq!(events(), ["create failed"]);
    assert_eq!(published(&state), first);
    // The old device still draws without a new renderer.
    assert_eq!(admit_as(&state, 0x10, 3), first);
    assert!(events().is_empty());
    release_all(&state);
}

#[test]
fn a_publication_during_creation_wins() {
    let state = Mutex::default();
    let outer = admit(
        &state,
        device(0x10),
        || {
            let inner = admit_as(&state, 0x10, 1);
            assert_eq!(name(inner), 1);
            make(2)
        },
        destroy,
        || event("published outer".into()),
    );
    assert_eq!(name(outer), 1);
    assert_eq!(events(), ["create 1", "published", "create 2", "destroy 2"]);
    assert_eq!(published(&state), outer);
    release_all(&state);
}

#[test]
fn release_destroys_then_forgets() {
    let state = Mutex::default();
    admit_as(&state, 0x10, 1);
    events();
    release_all(&state);
    assert_eq!(events(), ["destroy 1", "released"]);
    assert!(published(&state).is_null());
    assert!(published_for(&state, device(0x10)).is_null());

    // Release without a renderer still forgets the rest.
    release_all(&state);
    assert_eq!(events(), ["released"]);

    // The same device publishes again after a release.
    let again = admit_as(&state, 0x10, 2);
    assert_eq!(name(again), 2);
    assert_eq!(events(), ["create 2", "published"]);
    release_all(&state);
}

#[test]
fn device_lookup_never_creates() {
    let state = Mutex::default();
    assert!(published_for(&state, device(0x10)).is_null());
    assert!(published_for(&state, null_mut()).is_null());
    let first = admit_as(&state, 0x10, 1);
    events();
    assert_eq!(published_for(&state, device(0x10)), first);
    assert!(published_for(&state, device(0x11)).is_null());
    assert!(events().is_empty());
    release_all(&state);
}
