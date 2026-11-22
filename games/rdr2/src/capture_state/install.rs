// SPDX-License-Identifier: MIT
//! Native capture installation before the census can publish its hooks.
use super::{Capture, rsf_rdr2_capture_create};
use crate::diagnostics::{CAPTURE_PATH_CAPACITY, capture_directory};
use crate::platform::lock;
use std::ffi::{c_char, c_void};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_PATH: usize = CAPTURE_PATH_CAPACITY;
const INSTALL_VERSION: u32 = 1;
const FIXTURE_STOP_TIMEOUT_MS: u32 = 5000;

#[repr(C)]
struct Paths {
    directory: [u16; MAX_PATH],
    request: [u16; MAX_PATH],
    log: [u16; MAX_PATH],
}

impl Paths {
    fn new(directory: &[u16]) -> Self {
        fn path(directory: &[u16], suffix: &str) -> [u16; MAX_PATH] {
            let mut path = [0; MAX_PATH];
            for (slot, unit) in path
                .iter_mut()
                .zip(directory.iter().copied().chain(suffix.encode_utf16()))
            {
                *slot = unit;
            }
            path
        }
        Self {
            directory: path(directory, ""),
            request: path(directory, "\\native.request"),
            log: path(directory, "\\native-probe.log"),
        }
    }
}

#[repr(C)]
struct InstallOps {
    struct_size: u32,
    version: u32,
    create_state: unsafe extern "C" fn(capture: *mut Capture, paths: &Paths) -> bool,
    discard_state: unsafe extern "C" fn(),
    start_worker: unsafe extern "C" fn() -> *mut c_void,
    observe_d3d12: unsafe extern "C" fn(),
    observe_ngx: unsafe extern "C" fn(),
    wait_worker: unsafe extern "C" fn(thread: *mut c_void, timeout_ms: u32),
    close_worker: unsafe extern "C" fn(thread: *mut c_void),
    report: unsafe extern "C" fn(message: *const c_char),
}

impl InstallOps {
    unsafe fn start(&self, capture: NonNull<Capture>, paths: &Paths) -> Option<NonNull<c_void>> {
        if !unsafe { (self.create_state)(capture.as_ptr(), paths) } {
            return None;
        }
        let thread = NonNull::new(unsafe { (self.start_worker)() });
        if thread.is_none() {
            // The worker and census callbacks cannot have reached the prepared native state.
            unsafe { (self.discard_state)() };
        }
        thread
    }
}

const _: () = {
    assert!(size_of::<Paths>() == 3 * 260 * 2);
    assert!(size_of::<InstallOps>() == 8 + 8 * size_of::<usize>());
    assert!(std::mem::offset_of!(InstallOps, observe_d3d12) == 8 + 3 * size_of::<usize>());
    assert!(std::mem::offset_of!(InstallOps, wait_worker) == 8 + 5 * size_of::<usize>());
};

struct Worker {
    thread: NonNull<c_void>,
    ops: &'static InstallOps,
}

// The native leaves may wait on or close this owned Win32 thread handle from any thread.
unsafe impl Send for Worker {}

enum Installation {
    Game,
    Fixture(Option<Worker>),
}

static INSTALLED: Mutex<Option<Installation>> = Mutex::new(None);
static FIXTURE_STOP: AtomicBool = AtomicBool::new(false);

/// The native caller lends a NUL-terminated directory and a process-lived leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_install(
    directory: *const u16,
    ops: *const InstallOps,
    fixture: bool,
) -> bool {
    if directory.is_null() || ops.is_null() {
        return false;
    }
    let header = ops.cast::<u32>();
    if unsafe { header.read() } < size_of::<InstallOps>() as u32
        || unsafe { header.add(1).read() } != INSTALL_VERSION
    {
        return false;
    }
    let mut installed = lock(&INSTALLED);
    if installed.is_some() {
        return false;
    }
    let Some(directory) = (unsafe { capture_directory(directory) }) else {
        return false;
    };
    let paths = Paths::new(directory);
    let Some(capture) = NonNull::new(rsf_rdr2_capture_create()) else {
        return false;
    };
    let ops = unsafe { &*ops };
    let Some(thread) = (unsafe { ops.start(capture, &paths) }) else {
        unsafe { drop(Box::from_raw(capture.as_ptr())) };
        return false;
    };

    // Census callbacks and the hooks they install keep both owners for the life of the process.
    unsafe {
        (ops.observe_d3d12)();
        (ops.observe_ngx)();
    }
    *installed = Some(if fixture {
        Installation::Fixture(Some(Worker { thread, ops }))
    } else {
        unsafe { (ops.close_worker)(thread.as_ptr()) };
        Installation::Game
    });
    unsafe { (ops.report)(c"native capture installed".as_ptr()) };
    true
}

pub(super) fn fixture_stopping() -> bool {
    FIXTURE_STOP.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_stop_fixture() {
    let worker = {
        let mut installed = lock(&INSTALLED);
        let Some(Installation::Fixture(worker)) = installed.as_mut() else {
            return;
        };
        FIXTURE_STOP.store(true, Ordering::Release);
        worker.take()
    };
    if let Some(Worker { thread, ops }) = worker {
        // The hook owners stay live even if the worker takes longer than the fixture wait.
        unsafe {
            (ops.wait_worker)(thread.as_ptr(), FIXTURE_STOP_TIMEOUT_MS);
            (ops.close_worker)(thread.as_ptr());
        }
    }
}
