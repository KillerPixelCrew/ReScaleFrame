// SPDX-License-Identifier: GPL-3.0-only
use crate::sys::*;
use crate::sys::{
    RSF_FG_ABI_VERSION, RSF_FG_FIXED, rsf_fg_options, rsf_generation_provider, rsf_generation_setup,
};
use std::ffi::c_void;
use std::ptr::{from_ref, null_mut};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use windows_sys::Win32::System::{LibraryLoader::LoadLibraryW, Threading::CreateEventW};

type Raw = *mut c_void;

#[repr(C)]
struct UnknownVtable {
    query_interface: unsafe extern "system" fn(Raw, *const c_void, *mut Raw) -> i32,
    add_ref: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
}

#[repr(C)]
struct MockCom {
    vtable: &'static UnknownVtable,
    refs: AtomicU32,
}

impl MockCom {
    const fn new() -> Self {
        Self {
            vtable: &UNKNOWN,
            refs: AtomicU32::new(1),
        }
    }

    fn raw(&self) -> Raw {
        from_ref(self).cast_mut().cast()
    }

    fn retain(&self) -> Raw {
        self.refs.fetch_add(1, Relaxed);
        self.raw()
    }
}

static UNKNOWN: UnknownVtable = UnknownVtable {
    query_interface,
    add_ref,
    release,
};

unsafe extern "system" fn query_interface(_: Raw, _: *const c_void, _: *mut Raw) -> i32 {
    0x8000_4002_u32 as i32 // E_NOINTERFACE
}

unsafe extern "system" fn add_ref(raw: Raw) -> u32 {
    unsafe { &*raw.cast::<MockCom>() }
        .refs
        .fetch_add(1, Relaxed)
        + 1
}

unsafe extern "system" fn release(raw: Raw) -> u32 {
    unsafe { &*raw.cast::<MockCom>() }
        .refs
        .fetch_sub(1, Relaxed)
        - 1
}

static DEVICE: MockCom = MockCom::new();
static QUEUE: MockCom = MockCom::new();
static CHAIN: MockCom = MockCom::new();
static FENCE: MockCom = MockCom::new();

static CHECKS: rsf_gen12_checks = rsf_gen12_checks {
    setup: Some(setup_outputs),
    // This fixture drives begin, configure and markers. Refuse unexpected prepare work.
    frame: Some(refuse_frame),
    command: Some(refuse_command),
    streamline_loaded: Some(no_streamline),
    load_library: Some(load_library),
};

unsafe extern "C" fn setup_outputs(
    _: *const rsf_generation_setup,
    out: *mut Raw,
    chain: *mut Raw,
) -> i32 {
    unsafe {
        out.write(null_mut());
        chain.write(null_mut());
    }
    OK
}

unsafe extern "C" fn refuse_frame(_: *const crate::sys::rsf_fg_frame, _: Raw) -> i32 {
    INVALID
}

unsafe extern "C" fn refuse_command(_: Raw, _: Raw) -> i32 {
    INVALID
}

unsafe extern "C" fn no_streamline() -> u32 {
    0
}

unsafe extern "C" fn load_library(_: *const rsf_generation_setup, _: *const u16) -> Raw {
    // The owner retires these real module references without resolving SDK exports.
    let kernel32 = crate::wide(std::ffi::OsStr::new("kernel32.dll"));
    unsafe { LoadLibraryW(kernel32.as_ptr()) }
}

struct MockOps(rsf_xeg12_ops);

// The sole raw pointer is to the immutable CHECKS table.
unsafe impl Sync for MockOps {}

static OPS: MockOps = MockOps(rsf_xeg12_ops {
    check: &CHECKS,
    resolve: Some(resolve),
    close: Some(close),
    create_fg: Some(create_context),
    create_xell: Some(create_context),
    destroy_fg: Some(ok2),
    destroy_xell: Some(ok2),
    max_count: Some(max_count),
    bind_xell: Some(bind_xell),
    version: Some(version),
    create_chain: Some(create_chain),
    compose: Some(ok3),
    count: Some(ok3),
    enabled: Some(enabled),
    sleep_mode: Some(sleep_mode),
    sleep: Some(sleep),
    marker: Some(marker),
    present_id: Some(ok3),
    tag: Some(tag),
    constants: Some(constants),
    present_status: Some(present_status),
    create_fence: Some(create_fence),
    create_event: Some(create_event),
    drain: Some(drain),
});

unsafe extern "C" fn resolve(_: *const rsf_generation_setup, _: Raw, _: Raw, out: *mut Raw) -> i32 {
    unsafe { out.write(DEVICE.raw()) };
    OK
}

unsafe extern "C" fn close(_: Raw) {}

unsafe extern "C" fn create_context(_: Raw, _: Raw, out: *mut Raw) -> i32 {
    unsafe { out.write(DEVICE.raw()) };
    OK
}

unsafe extern "C" fn ok2(_: Raw, _: Raw) -> i32 {
    OK
}

unsafe extern "C" fn ok3(_: Raw, _: Raw, _: u32) -> i32 {
    OK
}

unsafe extern "C" fn max_count(_: Raw, _: Raw, out: *mut u32) -> i32 {
    unsafe { out.write(2) };
    OK
}

unsafe extern "C" fn bind_xell(_: Raw, _: Raw, _: Raw) -> i32 {
    OK
}

unsafe extern "C" fn version(_: Raw, _: *mut rsf_xeg12_version) {}

unsafe extern "C" fn create_chain(
    _: Raw,
    _: Raw,
    _: *const rsf_generation_setup,
    _: u32,
    out: *mut Raw,
) -> i32 {
    unsafe { out.write(CHAIN.retain()) };
    OK
}

unsafe extern "C" fn tag(
    _: Raw,
    _: Raw,
    _: Raw,
    _: u32,
    _: u32,
    _: *const crate::sys::rsf_backend_resource,
) -> i32 {
    INVALID
}

unsafe extern "C" fn constants(
    _: Raw,
    _: Raw,
    _: u32,
    _: *const crate::sys::rsf_fg_frame,
    _: u32,
) -> i32 {
    INVALID
}

unsafe extern "C" fn present_status(_: Raw, _: Raw, _: *mut rsf_xeg12_present) -> i32 {
    OK
}

unsafe extern "C" fn create_fence(_: Raw) -> Raw {
    FENCE.retain()
}

unsafe extern "C" fn create_event() -> Raw {
    unsafe { CreateEventW(null_mut(), 0, 0, null_mut()) }
}

unsafe extern "C" fn drain(queue: Raw, fence: Raw, event: Raw, _: u64) -> i32 {
    if queue == QUEUE.raw() && fence == FENCE.raw() && !event.is_null() {
        OK
    } else {
        INVALID
    }
}

const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const PRESENT_END: u32 = 5; // RSF_LATENCY_PRESENT_END in game_frame.h
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

unsafe extern "C" fn sleep_mode(_: Raw, _: Raw, interval: u32, _: u32) -> i32 {
    let mut sdk = SDK.lock().unwrap();
    sdk.interval = interval;
    sdk.updates += 1;
    OK
}

unsafe extern "C" fn sleep(_: Raw, _: Raw, id: u32) -> i32 {
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

unsafe extern "C" fn marker(_: Raw, _: Raw, id: u32, kind: u32) -> i32 {
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

unsafe extern "C" fn enabled(_: Raw, _: Raw, _: u32) -> i32 {
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

fn provider() -> &'static rsf_generation_provider {
    unsafe { &*rsf_core_xeg12_provider() }
}

impl Session {
    fn create(&mut self) {
        // The mock setup callback accepts the zeroed header used by this fixture.
        let mut setup: rsf_generation_setup = unsafe { std::mem::zeroed() };
        setup.chain.hwnd = CHAIN.raw();
        setup.chain.d3d12_device = DEVICE.raw();
        setup.chain.d3d12_queue = QUEUE.raw();
        setup.chain.ui_mode = RSF_UI_MODE_NONE;
        setup.chain.max_generated_frames = 2;
        let mut present = null_mut();
        assert_eq!(
            unsafe { rsf_core_xeg12_create(&OPS.0, &setup, &mut self.0, &mut present) },
            OK,
        );
        assert_eq!(present, CHAIN.raw());
    }

    fn begin(&self, source: u64) -> i32 {
        unsafe { (provider().begin_frame.unwrap())(self.0, source) }
    }

    fn configure(&self, frames: u32, interval: u32) -> i32 {
        let options = rsf_fg_options {
            struct_size: size_of::<rsf_fg_options>() as u32,
            abi_version: RSF_FG_ABI_VERSION,
            mode: RSF_FG_FIXED,
            generated_frames: frames,
            dynamic_target_fps: 0.0,
            reflex_mode: RSF_REFLEX_ON,
            frame_limit_us: interval,
        };
        unsafe { (provider().configure.unwrap())(self.0, &options) }
    }

    fn present_end(&self, source: u64) -> i32 {
        unsafe { (provider().marker.unwrap())(self.0, PRESENT_END, source, 0) }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { (provider().destroy.unwrap())(self.0) };
        }
    }
}

fn check() -> i32 {
    let mut session = Session(null_mut());
    session.create();
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
    drop(sdk);
    drop(session);
    for object in [&DEVICE, &QUEUE, &CHAIN, &FENCE] {
        assert_eq!(object.refs.load(Relaxed), 1);
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xell_fixture_run() -> i32 {
    crate::run(check)
}
