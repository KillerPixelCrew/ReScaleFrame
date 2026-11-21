// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::RSF_GAME_RENDER_SUBMISSION;
use std::cell::{Cell, RefCell};

/// The private FRHICommandListBase prefix the owner appends to.
#[repr(C)]
struct List {
    root: *mut Command,
    tail: *mut *mut Command,
    executing: u8,
    _padding: [u8; 3],
    count: u32,
    uid: u32,
}
const _: () = assert!(
    offset_of!(List, tail) == LIST_TAIL
        && offset_of!(List, executing) == LIST_EXECUTING
        && offset_of!(List, count) == LIST_COUNT
        && offset_of!(List, uid) == 0x18
);

impl List {
    fn new() -> Box<Self> {
        let mut list = Box::new(Self {
            root: null_mut(),
            tail: null_mut(),
            executing: 0,
            _padding: [0; 3],
            count: 0,
            uid: 1,
        });
        list.tail = &raw mut list.root;
        list
    }

    fn raw(&mut self) -> *mut c_void {
        (self as *mut Self).cast()
    }

    /// The engine iterator: saves Next before each execute, then resets the list.
    fn run(&mut self) {
        self.executing = 1;
        let list = self.raw();
        let mut command = self.root;
        while !command.is_null() {
            let next = unsafe { (*command).next };
            let execute = unsafe { (*command).execute }.expect("marker command function");
            unsafe { execute(list, command) };
            command = next;
        }
        self.executing = 0;
        self.root = null_mut();
        self.tail = &raw mut self.root;
        self.count = 0;
        self.uid += 1;
    }
}

struct Check {
    scopes: Cell<*mut Scopes>,
    /// (callback, begin, native_frame, scope_id, struct_size)
    observed: RefCell<Vec<(&'static str, u32, u64, u64, u32)>>,
    unload_refused: Cell<bool>,
    resolved: Cell<u32>,
    released: Cell<u32>,
    idle_at_release: Cell<bool>,
}

impl Check {
    fn new() -> Self {
        Self {
            scopes: Cell::new(null_mut()),
            observed: RefCell::new(Vec::new()),
            unload_refused: Cell::new(true),
            resolved: Cell::new(0),
            released: Cell::new(0),
            idle_at_release: Cell::new(false),
        }
    }

    fn user(&self) -> *mut c_void {
        (self as *const Self).cast_mut().cast()
    }

    fn frames(&self) -> Vec<u64> {
        self.observed.borrow().iter().map(|entry| entry.2).collect()
    }
}

fn check<'a>(user: *mut c_void) -> &'a Check {
    unsafe { &*user.cast::<Check>() }
}

fn record(name: &'static str, user: *mut c_void, scope: *const rsf_ac7_render_scope, begin: u32) {
    let check = check(user);
    let scope = unsafe { &*scope };
    check.observed.borrow_mut().push((
        name,
        begin,
        scope.native_frame,
        scope.scope_id,
        scope.struct_size,
    ));
    let refused = unsafe { rsf_ac7_render_scopes_destroy(check.scopes.get()) } == 0;
    check
        .unload_refused
        .set(check.unload_refused.get() && refused);
}

unsafe extern "C" fn state(
    user: *mut c_void,
    _: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    begin: u32,
) {
    record("state", user, scope, begin);
}

unsafe extern "C" fn pass(
    user: *mut c_void,
    _: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    begin: u32,
) {
    record("pass", user, scope, begin);
}

unsafe extern "C" fn lease_resolve(object: *mut c_void, scope: *mut rsf_ac7_render_scope) {
    let check = check(object);
    check.resolved.set(check.resolved.get() + 1);
    // Output published after begin was queued.
    unsafe { (*scope).native_frame += 100 };
}

unsafe extern "C" fn lease_release(object: *mut c_void) {
    let check = check(object);
    check.released.set(check.released.get() + 1);
    check
        .idle_at_release
        .set(unsafe { rsf_ac7_render_scopes_idle(check.scopes.get()) } != 0);
}

// The exception leaves without exceptions: Rust callbacks cannot throw.
unsafe extern "C" fn notify(
    callback: rsf_ac7_render_scope_fn,
    user: *mut c_void,
    list: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    begin: u32,
) -> bool {
    unsafe { callback.expect("nonnull callback")(user, list, scope, begin) };
    true
}

unsafe extern "C" fn resolve(
    lease: *const rsf_ac7_scope_lease,
    scope: *mut rsf_ac7_render_scope,
) -> bool {
    unsafe { (*lease).resolve.expect("nonnull resolve")((*lease).object, scope) };
    true
}

unsafe extern "C" fn release(lease: *const rsf_ac7_scope_lease) -> bool {
    unsafe { (*lease).release.expect("nonnull release")((*lease).object) };
    true
}

/// A resolve that threw before it changed anything.
unsafe extern "C" fn resolve_threw(
    _: *const rsf_ac7_scope_lease,
    _: *mut rsf_ac7_render_scope,
) -> bool {
    false
}

/// A release that threw after releasing.
unsafe extern "C" fn release_threw(lease: *const rsf_ac7_scope_lease) -> bool {
    unsafe { release(lease) };
    false
}

const CALLS: rsf_ac7_scope_calls = rsf_ac7_scope_calls {
    notify: Some(notify),
    resolve: Some(resolve),
    release: Some(release),
};

fn create(
    check: &Check,
    capacity: u32,
    state_callback: rsf_ac7_render_scope_fn,
    pass_callback: rsf_ac7_render_scope_fn,
    calls: &'static rsf_ac7_scope_calls,
) -> *mut Scopes {
    let mut scopes = null_mut();
    let created = unsafe {
        rsf_ac7_scope_owner_create(
            7,
            capacity,
            state_callback,
            pass_callback,
            check.user(),
            calls,
            &mut scopes,
        )
    };
    assert_eq!(created, 1);
    check.scopes.set(scopes);
    scopes
}

fn scene(native_frame: u64, view: u64) -> rsf_ac7_render_scope {
    let mut scope: rsf_ac7_render_scope = unsafe { zeroed() };
    scope.struct_size = size_of::<rsf_ac7_render_scope>() as u32;
    scope.session_id = 7;
    scope.family_key = 0x100;
    scope.view_key = view;
    scope.native_frame = native_frame;
    scope
}

fn open(scopes: *mut Scopes, list: &mut List, scope: &rsf_ac7_render_scope) -> *mut Ticket {
    let mut ticket = null_mut();
    if unsafe { rsf_ac7_render_scope_open(scopes, list.raw(), scope, &mut ticket) } == 0 {
        assert!(ticket.is_null());
        return null_mut();
    }
    ticket
}

fn close(ticket: *mut Ticket, list: &mut List) -> bool {
    unsafe { rsf_ac7_render_scope_close(ticket, list.raw()) != 0 }
}

fn idle(scopes: *mut Scopes) -> bool {
    unsafe { rsf_ac7_render_scopes_idle(scopes) != 0 }
}

fn destroy(scopes: *mut Scopes) -> bool {
    unsafe { rsf_ac7_render_scopes_destroy(scopes) != 0 }
}

fn quiesce(scopes: *mut Scopes) {
    unsafe { rsf_ac7_render_scopes_quiesce(scopes) };
}

fn current(scopes: *mut Scopes) -> Option<rsf_ac7_render_scope> {
    let mut out = scene(0, 0);
    (unsafe { rsf_ac7_render_scope_current(scopes, &mut out) } != 0).then_some(out)
}

// The first half of tests/ac7_render_scope.cpp.
#[test]
fn nested_scopes_restore_the_enclosing_scope_with_queued_identity() {
    let check = Check::new();
    let scopes = create(&check, 2, Some(state), None, &CALLS);
    let mut list = List::new();
    let mut frame = scene(41, 0x200);
    let outer = open(scopes, &mut list, &frame);
    frame.native_frame = 42;
    frame.view_key = 0x300;
    let inner = open(scopes, &mut list, &frame);
    assert!(!outer.is_null() && !inner.is_null());
    assert_eq!(list.count, 2);
    assert!(
        open(scopes, &mut list, &frame).is_null(),
        "bounded pending work"
    );
    assert!(close(inner, &mut list) && close(outer, &mut list));
    assert_eq!(list.count, 4);
    frame.native_frame = 900; // the CPU advanced; queued identity stays 41/42
    quiesce(scopes);
    assert!(!destroy(scopes), "unload waits for engine execution");
    assert!(!idle(scopes));
    assert!(
        open(scopes, &mut list, &frame).is_null(),
        "quiesce stops producers"
    );
    list.run();
    assert_eq!(check.frames(), [41, 42, 41, 0]);
    let observed = check.observed.borrow().clone();
    assert_eq!(
        observed.iter().map(|entry| entry.1).collect::<Vec<_>>(),
        [1, 1, 0, 0]
    );
    assert_eq!(observed[0].3, 1, "serials start at one");
    assert_eq!(observed[1].3, 2);
    assert_eq!(observed[3].4, 0, "no enclosing scope is a zeroed scope");
    assert!(check.unload_refused.get(), "callbacks finish before unload");
    assert!(current(scopes).is_none(), "closed stream has no stale view");
    assert!(idle(scopes));
    assert!(destroy(scopes));
}

// The second half of tests/ac7_render_scope.cpp.
#[test]
fn slate_scopes_need_their_owners_but_no_scene_view() {
    let check = Check::new();
    let scopes = create(&check, 2, Some(state), None, &CALLS);
    let mut list = List::new();
    let mut window = scene(0, 0);
    window.family_key = 0;
    window.role = RSF_GAME_RENDER_WINDOW;
    window.source_frame_id = 8;
    window.viewport_key = 0x100;
    window.window_key = 0x200;
    window.rhi_viewport_key = 0x300;
    window.pass_key = 0x400;
    let mut invalid = window;
    invalid.window_key = 0;
    assert!(open(scopes, &mut list, &invalid).is_null(), "window owner");
    invalid = window;
    invalid.rhi_viewport_key = 0;
    assert!(
        open(scopes, &mut list, &invalid).is_null(),
        "native viewport"
    );
    invalid = window;
    invalid.source_frame_id = 0;
    assert!(open(scopes, &mut list, &invalid).is_null(), "source frame");
    invalid = window;
    invalid.pass_key = 0;
    assert!(open(scopes, &mut list, &invalid).is_null(), "pass");
    let outer = open(scopes, &mut list, &window);
    let mut binding = window;
    binding.role = RSF_GAME_RENDER_TEXTURE_BINDING;
    binding.rhi_viewport_key = 0;
    invalid = binding;
    invalid.viewport_key = 0;
    assert!(
        open(scopes, &mut list, &invalid).is_null(),
        "source viewport"
    );
    let inner = open(scopes, &mut list, &binding);
    assert!(!outer.is_null() && !inner.is_null());
    assert!(close(inner, &mut list) && close(outer, &mut list));
    list.run();
    assert_eq!(check.observed.borrow().len(), 4);
    invalid = window;
    invalid.role = RSF_GAME_RENDER_SUBMISSION;
    assert!(
        open(scopes, &mut list, &invalid).is_null(),
        "scene work needs its view"
    );
    let mut frame = scene(0, 0);
    frame.family_key = 0;
    frame.role = RSF_GAME_RENDER_FRAME;
    assert!(
        open(scopes, &mut list, &frame).is_null(),
        "frame needs its source"
    );
    frame.source_frame_id = 9;
    let ticket = open(scopes, &mut list, &frame);
    assert!(!ticket.is_null() && close(ticket, &mut list));
    list.run();
    quiesce(scopes);
    assert!(destroy(scopes));
}

#[test]
fn pass_and_state_callbacks_see_the_ticket_and_the_selected_scope() {
    let check = Check::new();
    let scopes = create(&check, 4, Some(state), Some(pass), &CALLS);
    let mut list = List::new();
    let outer = open(scopes, &mut list, &scene(1, 0x10));
    let inner = open(scopes, &mut list, &scene(2, 0x20));
    assert!(close(inner, &mut list) && close(outer, &mut list));
    list.run();
    let names_frames: Vec<_> = check
        .observed
        .borrow()
        .iter()
        .map(|entry| (entry.0, entry.1, entry.2))
        .collect();
    assert_eq!(
        names_frames,
        [
            ("pass", 1, 1),
            ("state", 1, 1),
            ("pass", 1, 2),
            ("state", 1, 2),
            ("pass", 0, 2),
            ("state", 0, 1),
            ("pass", 0, 1),
            ("state", 0, 0),
        ]
    );
    quiesce(scopes);
    assert!(destroy(scopes));
}

#[test]
fn an_out_of_order_end_does_not_leave_a_released_scope_current() {
    let check = Check::new();
    let scopes = create(&check, 2, Some(state), Some(pass), &CALLS);
    let mut list = List::new();
    let outer = open(scopes, &mut list, &scene(1, 0x10));
    let inner = open(scopes, &mut list, &scene(2, 0x20));
    assert!(close(outer, &mut list) && close(inner, &mut list));
    list.run();
    assert_eq!(check.frames(), [1, 1, 2, 2, 2, 0]);
    assert!(current(scopes).is_none());
    quiesce(scopes);
    assert!(idle(scopes));
    assert!(destroy(scopes));
}

#[test]
fn current_reports_the_innermost_executed_scope() {
    let check = Check::new();
    let scopes = create(&check, 2, None, None, &CALLS);
    let mut list = List::new();
    let ticket = open(scopes, &mut list, &scene(5, 0x10));
    assert!(current(scopes).is_none(), "queued is not executed");
    list.run();
    let open_scope = current(scopes).expect("begin executed");
    assert_eq!((open_scope.native_frame, open_scope.scope_id), (5, 1));
    let mut short = scene(0, 0);
    short.struct_size -= 1;
    assert_eq!(
        unsafe { rsf_ac7_render_scope_current(scopes, &mut short) },
        0
    );
    assert_eq!(
        unsafe { rsf_ac7_render_scope_current(null_mut(), &mut short) },
        0
    );
    quiesce(scopes);
    assert!(
        !idle(scopes) && !destroy(scopes),
        "an open scope keeps the owner"
    );
    assert!(close(ticket, &mut list));
    list.run();
    assert!(current(scopes).is_none());
    assert!(destroy(scopes));
}

#[test]
fn leases_resolve_at_both_markers_and_release_before_the_owner_goes_idle() {
    let check = Check::new();
    let scopes = create(&check, 2, Some(state), Some(pass), &CALLS);
    let mut list = List::new();
    let lease = rsf_ac7_scope_lease {
        object: check.user(),
        resolve: Some(lease_resolve),
        release: Some(lease_release),
    };
    let mut ticket = null_mut();
    let opened = unsafe {
        rsf_ac7_render_scope_open_leased(scopes, list.raw(), &scene(1, 0x10), &lease, &mut ticket)
    };
    assert_eq!(opened, 1);
    list.run();
    assert_eq!((check.resolved.get(), check.released.get()), (1, 0));
    quiesce(scopes);
    assert!(close(ticket, &mut list));
    list.run();
    assert_eq!((check.resolved.get(), check.released.get()), (2, 1));
    assert!(
        !check.idle_at_release.get(),
        "release runs while the ticket is outstanding"
    );
    // Each marker saw the resolved scope; the end resolved again.
    assert_eq!(check.frames(), [101, 101, 201, 0]);
    assert!(idle(scopes));
    assert!(destroy(scopes));
}

#[test]
fn a_throwing_resolve_skips_the_stack_and_callbacks_but_retires_the_ticket() {
    static THREW: rsf_ac7_scope_calls = rsf_ac7_scope_calls {
        notify: Some(notify),
        resolve: Some(resolve_threw),
        release: Some(release_threw),
    };
    let check = Check::new();
    let scopes = create(&check, 1, Some(state), Some(pass), &THREW);
    let mut list = List::new();
    let lease = rsf_ac7_scope_lease {
        object: check.user(),
        resolve: Some(lease_resolve),
        release: Some(lease_release),
    };
    let mut ticket = null_mut();
    let opened = unsafe {
        rsf_ac7_render_scope_open_leased(scopes, list.raw(), &scene(1, 0x10), &lease, &mut ticket)
    };
    assert_eq!(opened, 1);
    assert!(close(ticket, &mut list));
    list.run();
    assert!(check.observed.borrow().is_empty());
    assert!(current(scopes).is_none());
    assert_eq!(
        check.released.get(),
        1,
        "a release that threw still retires the ticket"
    );
    quiesce(scopes);
    assert!(destroy(scopes));
}

#[test]
fn a_refused_end_resolve_restores_state_without_delivering_a_pass() {
    unsafe extern "C" fn resolve_begin_only(
        lease: *const rsf_ac7_scope_lease,
        scope: *mut rsf_ac7_render_scope,
    ) -> bool {
        if check(unsafe { (*lease).object }).resolved.get() == 0 {
            unsafe { resolve(lease, scope) }
        } else {
            // A failed resolve may have partly modified the record.
            unsafe { (*scope).scope_id = 0 };
            false
        }
    }

    static CALLS_WITH_REFUSED_END: rsf_ac7_scope_calls = rsf_ac7_scope_calls {
        resolve: Some(resolve_begin_only),
        ..CALLS
    };
    let check = Check::new();
    let scopes = create(&check, 1, Some(state), Some(pass), &CALLS_WITH_REFUSED_END);
    let mut list = List::new();
    let lease = rsf_ac7_scope_lease {
        object: check.user(),
        resolve: Some(lease_resolve),
        release: Some(lease_release),
    };
    let mut ticket = null_mut();
    assert_eq!(
        unsafe {
            rsf_ac7_render_scope_open_leased(
                scopes,
                list.raw(),
                &scene(1, 0x10),
                &lease,
                &mut ticket,
            )
        },
        1
    );
    assert!(close(ticket, &mut list));
    list.run();
    assert_eq!(check.frames(), [101, 101, 0]);
    assert_eq!(check.released.get(), 1);
    assert!(current(scopes).is_none());
    quiesce(scopes);
    assert!(destroy(scopes));
}

#[test]
fn a_lease_without_functions_and_an_unleased_open_are_equivalent() {
    let check = Check::new();
    let scopes = create(&check, 1, Some(state), None, &CALLS);
    let mut list = List::new();
    let mut ticket = null_mut();
    let opened = unsafe {
        rsf_ac7_render_scope_open_leased(
            scopes,
            list.raw(),
            &scene(3, 0x10),
            &NO_LEASE,
            &mut ticket,
        )
    };
    assert_eq!(opened, 1);
    assert!(close(ticket, &mut list));
    list.run();
    assert_eq!(check.frames(), [3, 0]);
    quiesce(scopes);
    assert!(destroy(scopes));
}

#[test]
fn refused_appends_leave_the_list_and_the_owner_unchanged() {
    let check = Check::new();
    let scopes = create(&check, 2, None, None, &CALLS);
    let frame = scene(1, 0x10);

    let mut executing = List::new();
    executing.executing = 1;
    assert!(open(scopes, &mut executing, &frame).is_null());
    assert_eq!(executing.count, 0);

    let mut no_tail = List::new();
    no_tail.tail = null_mut();
    assert!(open(scopes, &mut no_tail, &frame).is_null());

    let mut occupied = List::new();
    let mut stray = Command::new(true);
    occupied.root = &raw mut stray;
    assert!(open(scopes, &mut occupied, &frame).is_null());
    assert_eq!(occupied.count, 0);

    let mut full = List::new();
    full.count = u32::MAX;
    assert!(open(scopes, &mut full, &frame).is_null());
    assert_eq!(full.count, u32::MAX);

    // A refused close keeps the ticket open for another list; a second close refuses.
    let mut list = List::new();
    let ticket = open(scopes, &mut list, &frame);
    assert!(!ticket.is_null());
    let mut refusing = List::new();
    refusing.executing = 1;
    assert!(!close(ticket, &mut refusing));
    assert!(close(ticket, &mut list));
    assert!(!close(ticket, &mut list));
    assert!(!close(null_mut(), &mut list));
    assert_eq!(list.count, 2);
    list.run();
    quiesce(scopes);
    assert!(idle(scopes), "refusals leaked no ticket");
    assert!(destroy(scopes));
}

#[test]
fn open_refuses_foreign_short_or_unidentified_scopes() {
    let check = Check::new();
    let scopes = create(&check, 2, None, None, &CALLS);
    let mut list = List::new();
    let mut foreign = scene(1, 0x10);
    foreign.session_id = 8;
    assert!(open(scopes, &mut list, &foreign).is_null());
    let mut short = scene(1, 0x10);
    short.struct_size -= 1;
    assert!(open(scopes, &mut list, &short).is_null());
    assert!(open(scopes, &mut list, &scene(1, 0)).is_null(), "no view");
    let mut no_family = scene(1, 0x10);
    no_family.family_key = 0;
    assert!(open(scopes, &mut list, &no_family).is_null());
    let mut ticket = null_mut();
    let frame = scene(1, 0x10);
    assert_eq!(
        unsafe { rsf_ac7_render_scope_open(null_mut(), list.raw(), &frame, &mut ticket) },
        0
    );
    assert_eq!(
        unsafe { rsf_ac7_render_scope_open(scopes, null_mut(), &frame, &mut ticket) },
        0
    );
    assert_eq!(
        unsafe { rsf_ac7_render_scope_open(scopes, list.raw(), null(), &mut ticket) },
        0
    );
    assert_eq!(
        unsafe { rsf_ac7_render_scope_open(scopes, list.raw(), &frame, null_mut()) },
        0
    );
    assert_eq!(list.count, 0);
    quiesce(scopes);
    assert!(destroy(scopes));
}

#[test]
fn create_checks_its_arguments() {
    let mut scopes = null_mut();
    let make = |session, capacity, calls: *const rsf_ac7_scope_calls, out: *mut *mut Scopes| unsafe {
        rsf_ac7_scope_owner_create(session, capacity, None, None, null_mut(), calls, out)
    };
    assert_eq!(make(7, 1, &CALLS, null_mut()), 0);
    assert_eq!(make(0, 1, &CALLS, &mut scopes), 0);
    assert_eq!(make(7, 0, &CALLS, &mut scopes), 0);
    assert_eq!(make(7, CAPACITY_LIMIT + 1, &CALLS, &mut scopes), 0);
    assert!(scopes.is_null());
    assert_eq!(make(7, 1, null(), &mut scopes), 0);
    let incomplete = rsf_ac7_scope_calls {
        release: None,
        ..CALLS
    };
    assert_eq!(make(7, 1, &incomplete, &mut scopes), 0);
    assert!(scopes.is_null());
    assert_eq!(make(7, CAPACITY_LIMIT, &CALLS, &mut scopes), 1);
    assert!(!scopes.is_null());
    assert!(!destroy(scopes), "an accepting owner is not idle");
    quiesce(scopes);
    assert!(destroy(scopes));
    // Null owners: idle and destroyed, quiesce ignored.
    quiesce(null_mut());
    assert!(idle(null_mut()) && destroy(null_mut()));
}
