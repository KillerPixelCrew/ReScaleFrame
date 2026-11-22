// SPDX-License-Identifier: GPL-3.0-only
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

unsafe extern "C" {
    fn rsf_xell_fixture_create(session: *mut *mut c_void) -> i32;
    fn rsf_xell_fixture_destroy(session: *mut c_void);
    fn rsf_xell_fixture_configure(session: *mut c_void, frames: u32, interval: u32) -> i32;
    fn rsf_xell_fixture_begin(session: *mut c_void, source: u64) -> i32;
    fn rsf_xell_fixture_present_end(session: *mut c_void, source: u64) -> i32;
}

const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const PRESENT_END: u32 = 5;
const TIMEOUT: Duration = Duration::from_secs(2);

struct FakeSdk {
    entered: bool,
    marked: bool,
    fail_enable: bool,
    interval: u32,
    updates: u32,
}

static SDK: Mutex<FakeSdk> = Mutex::new(FakeSdk {
    entered: false,
    marked: false,
    fail_enable: false,
    interval: 0,
    updates: 0,
});
static CHANGED: Condvar = Condvar::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_xell_fixture_sleep_mode(interval: u32) -> i32 {
    let mut sdk = SDK.lock().unwrap();
    sdk.interval = interval;
    sdk.updates += 1;
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xell_fixture_sleep(id: u32) -> i32 {
    match id {
        1 => OK,
        2 => {
            let mut sdk = SDK.lock().unwrap();
            sdk.entered = true;
            CHANGED.notify_all();
            let (sdk, _) = CHANGED
                .wait_timeout_while(sdk, TIMEOUT, |sdk| !sdk.marked)
                .unwrap();
            if sdk.marked { OK } else { INVALID }
        }
        _ => INVALID,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xell_fixture_marker(id: u32, kind: u32) -> i32 {
    if kind != PRESENT_END {
        return INVALID;
    }
    match id {
        1 => {
            SDK.lock().unwrap().marked = true;
            CHANGED.notify_all();
            OK
        }
        2 => OK,
        _ => INVALID,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xell_fixture_enabled() -> i32 {
    if SDK.lock().unwrap().fail_enable {
        INVALID
    } else {
        OK
    }
}

struct Session(*mut c_void);

// Only the owner's begin call is moved to the worker, and it joins before
// destruction. The production owner admits marker/configure concurrently.
unsafe impl Sync for Session {}

impl Session {
    fn begin(&self, source: u64) -> i32 {
        unsafe { rsf_xell_fixture_begin(self.0, source) }
    }

    fn configure(&self, frames: u32, interval: u32) -> i32 {
        unsafe { rsf_xell_fixture_configure(self.0, frames, interval) }
    }

    fn present_end(&self, source: u64) -> i32 {
        unsafe { rsf_xell_fixture_present_end(self.0, source) }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { rsf_xell_fixture_destroy(self.0) };
        }
    }
}

fn check() -> i32 {
    let mut session = Session(null_mut());
    assert_eq!(unsafe { rsf_xell_fixture_create(&mut session.0) }, OK);
    assert_eq!(session.configure(1, 0), OK);
    assert_eq!(session.begin(10), OK);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| session.begin(11));
        let started = {
            let (sdk, _) = CHANGED
                .wait_timeout_while(SDK.lock().unwrap(), TIMEOUT, |sdk| !sdk.entered)
                .unwrap();
            sdk.entered
        };
        assert!(started);
        let premature = session.present_end(11);
        let busy = session.configure(1, 1000);
        let unchanged_sleep = session.configure(2, 0);
        assert_eq!(session.present_end(10), OK);
        assert_eq!(worker.join().unwrap(), OK);
        assert_eq!(session.present_end(11), OK);
        assert_eq!(session.begin(11), INVALID);
        assert_eq!((premature, busy), (NOT_READY, NOT_READY));
        assert_eq!((unchanged_sleep, SDK.lock().unwrap().updates), (OK, 1));
    });

    // XeLL may accept a limit before a following XeFG operation fails.
    SDK.lock().unwrap().fail_enable = true;
    assert_eq!(session.configure(2, 2000), FEATURE_FAILED);
    {
        let mut sdk = SDK.lock().unwrap();
        assert_eq!((sdk.interval, sdk.updates), (2000, 2));
        sdk.fail_enable = false;
    }
    assert_eq!(session.configure(2, 0), OK);
    let sdk = SDK.lock().unwrap();
    assert_eq!((sdk.interval, sdk.updates), (0, 3));
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xell_fixture_run() -> i32 {
    crate::run(check)
}
