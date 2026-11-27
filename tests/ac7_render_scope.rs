// SPDX-License-Identifier: MIT
//! Drives real exceptions through the C++ callback leaves on the native iterator ABI.
mod ac7_render_scope_fixture_abi;

use ac7_render_scope_fixture_abi::rsf_ac7_fixture_lease as Lease;
use std::ffi::{c_int, c_void};
use std::ptr::{from_mut, null_mut};

type Callback = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, u32);

unsafe extern "C" {
    fn rsf_ac7_fixture_throwing_pass(_: *mut c_void, _: *mut c_void, _: *const c_void, _: u32);
    fn rsf_ac7_fixture_enqueue(owner: *mut c_void, list: *mut c_void, lease: *mut Lease) -> c_int;
    fn rsf_ac7_fixture_native_frame(scope: *const c_void) -> u64;
    fn rsf_ac7_fixture_current(owner: *mut c_void) -> c_int;
    fn rsf_ac7_render_scopes_create_passes(
        session: u64,
        capacity: u32,
        state: Callback,
        pass: Callback,
        user: *mut c_void,
        out: *mut *mut c_void,
    ) -> c_int;
    fn rsf_ac7_render_scopes_quiesce(owner: *mut c_void);
    fn rsf_ac7_render_scopes_destroy(owner: *mut c_void) -> c_int;
}

#[repr(C)]
struct Command {
    next: *mut Command,
    execute: unsafe extern "C" fn(*mut c_void, *mut Command),
}
const _: () = assert!(
    std::mem::offset_of!(Command, next) == 0 && std::mem::offset_of!(Command, execute) == 8
);

#[repr(C)]
struct List {
    root: *mut Command,
    tail: *mut *mut Command,
    executing: u8,
    padding: [u8; 3],
    count: u32,
    uid: u32,
}
const _: () = assert!(
    std::mem::offset_of!(List, tail) == 8
        && std::mem::offset_of!(List, executing) == 0x10
        && std::mem::offset_of!(List, count) == 0x14
        && std::mem::offset_of!(List, uid) == 0x18
);

unsafe fn run(list: *mut List) {
    unsafe {
        (*list).executing = 1;
        let mut command = (*list).root;
        while !command.is_null() {
            let next = (*command).next;
            ((*command).execute)(list.cast(), command);
            command = next;
        }
        (*list).executing = 0;
        (*list).root = null_mut();
        (*list).tail = &raw mut (*list).root;
        (*list).count = 0;
        (*list).uid += 1;
    }
}

struct Check {
    owner: *mut c_void,
    observed: Vec<u64>,
    unload_refused: bool,
}

unsafe extern "C" fn notify(user: *mut c_void, _: *mut c_void, scope: *const c_void, _: u32) {
    let check = unsafe { &mut *user.cast::<Check>() };
    check
        .observed
        .push(unsafe { rsf_ac7_fixture_native_frame(scope) });
    check.unload_refused &= unsafe { rsf_ac7_render_scopes_destroy(check.owner) } == 0;
}

fn expect(ok: bool, message: &str) -> bool {
    if !ok {
        eprintln!("{message}");
    }
    ok
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_render_scope_fixture() -> c_int {
    let mut check = Check {
        owner: null_mut(),
        observed: Vec::new(),
        unload_refused: true,
    };
    let check_ptr = from_mut(&mut check);
    let mut owner = null_mut();
    if !expect(
        unsafe {
            rsf_ac7_render_scopes_create_passes(
                7,
                2,
                notify,
                rsf_ac7_fixture_throwing_pass,
                check_ptr.cast(),
                &mut owner,
            )
        } != 0,
        "create pass scopes",
    ) {
        return 1;
    }
    unsafe { (*check_ptr).owner = owner };
    let mut list = List {
        root: null_mut(),
        tail: null_mut(),
        executing: 0,
        padding: [0; 3],
        count: 0,
        uid: 1,
    };
    let list = from_mut(&mut list);
    unsafe { (*list).tail = &raw mut (*list).root };
    let mut lease = Lease {
        resolves: 0,
        releases: 0,
        throw_resolve: false,
    };
    let mut ok = expect(
        unsafe { rsf_ac7_fixture_enqueue(owner, list.cast(), &raw mut lease) } != 0,
        "queue leased scope",
    );
    unsafe { run(list) };
    ok &= expect(
        unsafe { &*check_ptr }.observed == [105, 0],
        "pass exception keeps state callbacks",
    );
    ok &= expect(
        lease.resolves == 2 && lease.releases == 1,
        "lease resolved per marker, released once",
    );
    lease.throw_resolve = true;
    ok &= expect(
        unsafe { rsf_ac7_fixture_enqueue(owner, list.cast(), &raw mut lease) } != 0,
        "queue throwing lease",
    );
    unsafe { run(list) };
    let check = unsafe { &*check_ptr };
    ok &= expect(
        check.observed.len() == 2,
        "throwing resolve skips both markers",
    );
    ok &= expect(
        lease.resolves == 4 && lease.releases == 2,
        "throwing release still retires",
    );
    ok &= expect(check.unload_refused, "callbacks must finish before unload");
    ok &= expect(
        unsafe { rsf_ac7_fixture_current(owner) } == 0,
        "no scope left open",
    );
    unsafe { rsf_ac7_render_scopes_quiesce(owner) };
    ok &= expect(
        unsafe { rsf_ac7_render_scopes_destroy(owner) } != 0,
        "exception paths retired every ticket",
    );
    c_int::from(!ok)
}
