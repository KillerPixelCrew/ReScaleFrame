// SPDX-License-Identifier: MIT
use super::{ABI_MISMATCH, BUSY, INVALID_ARGUMENT, NATIVE_REFUSED, NOT_READY, OK, run, wide};
use rsf_game_sys::plugin_session_abi::{
    RSF_PLUGIN_SESSION_ABI_VERSION, rsf_plugin_session_options,
};
use rsf_game_sys::*;
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

type Handle = *mut c_void;

#[repr(C)]
struct Functions {
    prepare: unsafe extern "C" fn(*const rsf_plugin_session_options, *mut Handle) -> i32,
    start: unsafe extern "C" fn(Handle) -> i32,
    quiesce: unsafe extern "C" fn(Handle) -> i32,
    status: unsafe extern "C" fn(Handle, *mut rsf_game_renderer_status) -> i32,
    stop: unsafe extern "C" fn(Handle) -> i32,
}

struct Log<'a> {
    functions: &'a Functions,
    slot: *const Handle,
    calls: Cell<u32>,
    wrong_result: Cell<bool>,
}

fn status() -> rsf_game_renderer_status {
    rsf_game_renderer_status {
        struct_size: size_of::<rsf_game_renderer_status>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        prepared: 0,
        active: 0,
        rendering_ready: 0,
        reason: null(),
    }
}

unsafe extern "C" fn log(user: *mut c_void, operation: *const c_char) {
    let state = unsafe { &*user.cast::<Log<'_>>() };
    let session = unsafe { state.slot.read() };
    let result = if unsafe { CStr::from_ptr(operation) } == c"start" {
        unsafe { (state.functions.status)(session, &mut status()) }
    } else {
        unsafe { (state.functions.quiesce)(session) }
    };
    state.calls.set(state.calls.get() + 1);
    state
        .wrong_result
        .set(state.wrong_result.get() || result != BUSY);
}

// Only the size field is initialized, with the alignment of the options record.
#[repr(C, align(8))]
struct ShortOptions(u32);

#[unsafe(no_mangle)]
extern "C" fn rsf_test_plugin_session(functions: &Functions) -> i32 {
    run(|| {
        let args: Vec<_> = std::env::args_os().collect();
        if args.len() != 2 {
            return 2;
        }
        let path = std::path::absolute(Path::new(&args[1])).expect("resolve fixture path");
        let leaf = wide(path.file_name().expect("fixture DLL name"));
        let path = wide(path.as_os_str());
        let loaded = || !unsafe { GetModuleHandleW(leaf.as_ptr()) }.is_null();
        let mut session = std::ptr::dangling_mut::<c_void>();
        let slot = &raw mut session;
        let log_state = Log {
            functions,
            slot,
            calls: Cell::new(0),
            wrong_result: Cell::new(false),
        };
        let mut probe = rsf_game_probe {
            struct_size: size_of::<rsf_game_probe>() as u32,
            pe_machine: 0,
            reserved: 0,
            executable_name_utf8: null(),
            sha256_hex: null(),
        };
        let probe = &raw mut probe;
        let mut options = rsf_plugin_session_options {
            struct_size: size_of::<rsf_plugin_session_options>() as u32,
            abi_version: RSF_PLUGIN_SESSION_ABI_VERSION,
            plugin_path: path.as_ptr(),
            probe,
            services: rsf_game_sys::session::host_services(0, Some(log)),
        };
        options.services.user = (&raw const log_state).cast_mut().cast();
        let prepare = |options, expected| {
            assert_eq!(unsafe { (functions.prepare)(options, slot) }, expected);
        };

        assert!(!loaded());
        let short = ShortOptions(size_of::<u32>() as u32);
        prepare((&raw const short).cast(), INVALID_ARGUMENT);
        assert!(session.is_null());
        let invalid_path = wide(std::ffi::OsStr::new("1:/fixture.dll"));
        options.plugin_path = invalid_path.as_ptr();
        prepare(&options, INVALID_ARGUMENT);
        assert!(session.is_null());
        options.plugin_path = path.as_ptr();
        options.abi_version += 1;
        prepare(&options, ABI_MISMATCH);
        assert!(session.is_null());
        options.abi_version -= 1;

        unsafe { (*probe).pe_machine = 1 };
        prepare(&options, NATIVE_REFUSED);
        assert!(session.is_null() && !loaded());
        unsafe { (*probe).pe_machine = 2 };
        prepare(&options, NOT_READY);
        assert!(session.is_null() && !loaded());
        unsafe { (*probe).pe_machine = 0 };

        prepare(&options, OK);
        assert!(!session.is_null());
        options.services.log = None; // The session must retain its own services copy.
        let duplicate = |options: &rsf_plugin_session_options| {
            let mut other = session;
            assert_eq!(unsafe { (functions.prepare)(options, &mut other) }, BUSY);
            assert!(other.is_null());
        };
        duplicate(&options);
        unsafe {
            assert_eq!((functions.start)(session), OK);
            assert_eq!((functions.start)(session), OK);
            assert_eq!((functions.status)(session, &mut status()), OK);
            assert_eq!((functions.stop)(session), BUSY);
            assert_eq!((functions.quiesce)(session), OK);
            assert_eq!((functions.start)(session), NOT_READY);
            assert_eq!((functions.stop)(session), OK);
        }
        assert!(!loaded());

        options.services.session_id = 2;
        options.services.log = Some(log);
        prepare(&options, BUSY);
        assert!(!session.is_null() && loaded());
        let mut other = session;
        unsafe {
            assert_eq!((functions.prepare)(&options, &mut other), BUSY);
            assert!(other.is_null());
            assert_eq!((functions.start)(session), NOT_READY);
            assert_eq!((functions.quiesce)(session), OK);
            assert_eq!((functions.stop)(session), BUSY);
            assert_eq!((functions.status)(session, &mut status()), OK);
            assert_eq!((functions.stop)(session), OK);
        }
        assert!(!loaded());

        options.services.session_id = 3;
        prepare(&options, OK);
        assert!(!session.is_null());
        unsafe {
            assert_eq!((functions.start)(session), OK);
            assert_eq!((functions.quiesce)(session), BUSY);
            assert_eq!((functions.start)(session), NOT_READY);
            assert_eq!((functions.stop)(session), BUSY);
            assert_eq!((functions.status)(session, &mut status()), OK);
            assert_eq!((functions.quiesce)(session), OK);
            assert_eq!((functions.stop)(session), OK);
            assert_eq!((functions.stop)(null_mut()), OK);
        }
        assert!(!loaded());
        assert_eq!(log_state.calls.get(), 5);
        assert!(!log_state.wrong_result.get());
        OK
    })
}
