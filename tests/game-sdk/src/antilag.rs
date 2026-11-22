// SPDX-License-Identifier: MIT
use crate::sys::*;
use std::ffi::c_void;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct DriverState {
    initializes: u32,
    updates: u32,
    ends: u32,
    shutdowns: u32,
    api: u32,
    update_api: u32,
    fps: u32,
    enabled: bool,
    attached: bool,
    fail_initialize: bool,
    fail_update: bool,
    sleeping: bool,
}

#[derive(Default)]
struct Driver {
    state: Mutex<DriverState>,
    change: Condvar,
    block_second_update: bool,
}

unsafe extern "C" fn initialize(raw: *mut c_void, api: u32, _: *mut c_void) -> u32 {
    let driver = unsafe { &*raw.cast::<Driver>() };
    let mut state = driver.state.lock().unwrap();
    state.initializes += 1;
    state.api = api;
    u32::from(!state.fail_initialize)
}

unsafe extern "C" fn update(raw: *mut c_void, api: u32, enabled: u32, fps: u32) -> u32 {
    let driver = unsafe { &*raw.cast::<Driver>() };
    let mut state = driver.state.lock().unwrap();
    state.updates += 1;
    state.update_api = api;
    state.enabled = enabled != 0;
    state.fps = fps;
    if driver.block_second_update && state.updates == 2 {
        state.sleeping = true;
        driver.change.notify_all();
        let (state, _) = driver
            .change
            .wait_timeout_while(state, Duration::from_secs(3), |s| !s.attached)
            .unwrap();
        return u32::from(state.attached);
    }
    u32::from(!state.fail_update)
}

unsafe extern "C" fn end_render(raw: *mut c_void, _: *mut c_void, attach: u32) -> u32 {
    let driver = unsafe { &*raw.cast::<Driver>() };
    let mut state = driver.state.lock().unwrap();
    state.ends += 1;
    state.attached = attach != 0;
    driver.change.notify_all();
    1
}

unsafe extern "C" fn shutdown(raw: *mut c_void) {
    let driver = unsafe { &*raw.cast::<Driver>() };
    driver.state.lock().unwrap().shutdowns += 1;
}

static CALLBACKS: AntiLagCallbacks = AntiLagCallbacks {
    initialize: Some(initialize),
    update: Some(update),
    end_render: Some(end_render),
    shutdown: Some(shutdown),
};

struct Owner<'a> {
    context: *mut AntiLagContext,
    _driver: &'a Driver,
}

// The core accepts overlapping end-render and pacing calls. The callback data
// lives for the owner and serializes mutation with its own mutex.
unsafe impl Sync for Owner<'_> {}

impl<'a> Owner<'a> {
    fn new(driver: &'a Driver) -> Self {
        let context = unsafe {
            rsf_core_antilag_create(&CALLBACKS, std::ptr::from_ref(driver).cast_mut().cast())
        };
        assert!(!context.is_null());
        Self {
            context,
            _driver: driver,
        }
    }

    fn state(&self) -> AntiLagState {
        let mut status = AntiLagState::default();
        unsafe { rsf_core_antilag_state(self.context, &mut status) };
        status
    }

    fn bind(&self, api: u32, device: *mut c_void, vendor: u32) -> bool {
        unsafe { rsf_core_antilag_bind(self.context, api, device, vendor) != 0 }
    }

    fn pace(&self, source: u64, mode: u32, limit: u32, other_owner: u32) -> bool {
        unsafe { rsf_core_antilag_pace(self.context, source, mode, limit, other_owner) != 0 }
    }

    fn end(&self, source: u64, chain: *mut c_void, generated: bool) -> bool {
        unsafe {
            rsf_core_antilag_end_render(self.context, source, chain, u32::from(generated)) != 0
        }
    }
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        unsafe { rsf_core_antilag_destroy(self.context) };
    }
}

fn check() -> i32 {
    const AMD: u32 = 0x1002;
    const NONE: u32 = 0;
    const REFLEX: u32 = 1;
    const XELL: u32 = 2;
    let mut device = 0;
    let mut chain = 0;
    let device = (&raw mut device).cast();
    let chain = (&raw mut chain).cast();
    let driver = Driver::default();
    let owner = Owner::new(&driver);

    owner.bind(12, device, 0x10de);
    assert_eq!(owner.state().available, 0);
    assert_eq!(driver.state.lock().unwrap().initializes, 0);
    owner.bind(11, device, 0x8086);
    assert!(!owner.pace(1, 1, 0, NONE));
    assert_eq!(driver.state.lock().unwrap().updates, 0);
    owner.bind(12, device, AMD);
    assert_eq!(owner.state().available, 1);
    {
        let state = driver.state.lock().unwrap();
        assert_eq!((state.initializes, state.api), (1, 12));
    }
    assert!(owner.pace(1, 1, 10000, NONE));
    assert!(!owner.pace(1, 1, 10000, NONE));
    {
        let state = driver.state.lock().unwrap();
        assert!(state.enabled);
        assert_eq!((state.updates, state.fps, state.update_api), (1, 100, 12));
    }
    assert!(owner.end(1, chain, true));
    assert!(!owner.end(1, chain, true));
    {
        let state = driver.state.lock().unwrap();
        assert!(state.attached);
        assert_eq!(state.ends, 1);
    }
    assert!(owner.end(2, chain, false));
    assert!(!driver.state.lock().unwrap().attached);
    assert!(!owner.pace(2, 0, 0, NONE));
    {
        let state = driver.state.lock().unwrap();
        assert!(!state.enabled && state.fps == 0);
    }
    for (source, mode, pacing) in [(3, 1, XELL), (4, 2, REFLEX)] {
        assert!(!owner.pace(source, mode, 0, pacing));
        assert!(!driver.state.lock().unwrap().enabled);
    }
    owner.bind(11, device, AMD);
    assert_eq!(driver.state.lock().unwrap().api, 11);
    assert!(owner.pace(5, 2, 20000, NONE));
    {
        let state = driver.state.lock().unwrap();
        assert_eq!((state.fps, state.update_api), (50, 11));
    }
    assert!(!owner.end(5, chain, true));
    driver.state.lock().unwrap().fail_update = true;
    assert!(!owner.pace(6, 1, 0, NONE));
    assert_eq!((owner.state().available, owner.state().refused), (0, 1));
    let updates = driver.state.lock().unwrap().updates;
    assert!(!owner.pace(7, 1, 0, NONE));
    assert_eq!(driver.state.lock().unwrap().updates, updates);

    let unsupported = Driver::default();
    unsupported.state.lock().unwrap().fail_initialize = true;
    let missing = Owner::new(&unsupported);
    missing.bind(12, device, AMD);
    assert_eq!((missing.state().available, missing.state().refused), (0, 1));
    assert!(!missing.pace(1, 1, 0, NONE));

    let reset = Driver::default();
    let resetting = Owner::new(&reset);
    resetting.bind(12, device, AMD);
    assert!(resetting.pace(1, 1, 0, NONE));
    let before_shutdown = reset.state.lock().unwrap().shutdowns;
    reset.state.lock().unwrap().fail_update = true;
    unsafe { rsf_core_antilag_reset_sources(resetting.context) };
    {
        let state = reset.state.lock().unwrap();
        assert_eq!(
            (state.shutdowns, state.update_api),
            (before_shutdown + 1, 12)
        );
    }
    let state = resetting.state();
    assert_eq!((state.available, state.enabled, state.refused), (0, 0, 1));

    let sleeper = Driver {
        block_second_update: true,
        ..Default::default()
    };
    let concurrent = Owner::new(&sleeper);
    concurrent.bind(12, device, AMD);
    assert!(concurrent.pace(1, 1, 0, NONE));
    std::thread::scope(|scope| {
        let pacing = scope.spawn(|| concurrent.pace(2, 1, 0, NONE));
        let state = sleeper.state.lock().unwrap();
        let (state, _) = sleeper
            .change
            .wait_timeout_while(state, Duration::from_secs(2), |s| !s.sleeping)
            .unwrap();
        let sleeping = state.sleeping;
        drop(state);
        assert!(sleeping);
        let mut other = 0;
        assert!(!concurrent.bind(12, (&raw mut other).cast(), AMD));
        assert_eq!(concurrent.state().updates, 1);
        assert!(concurrent.end(1, chain, true));
        assert!(pacing.join().unwrap());
    });
    assert_eq!(
        (concurrent.state().updates, concurrent.state().end_markers),
        (2, 1)
    );
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_antilag_fixture() -> i32 {
    crate::run(check)
}
