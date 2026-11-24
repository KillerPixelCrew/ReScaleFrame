// SPDX-License-Identifier: GPL-3.0-only
//! The xell_sleep_concurrency_contract fixture (test-fixtures only): the XeSS FG owner over CPU
//! doubles of its SDK entries, proving XeLL sleep, marker and configure admission.
use super::*;
use crate::sys::{RSF_FG_ABI_VERSION, RSF_REFLEX_ON, RSF_UI_MODE_NONE};
use rsf_game_sys::backend_result::INVALID_ARGUMENT as INVALID;
use std::ptr::{from_ref, null_mut};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use windows_sys::Win32::System::{LibraryLoader::LoadLibraryW, Threading::CreateEventW};

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

static CHECKS: Checks = Checks {
    setup: setup_outputs,
    // This fixture drives begin, configure and markers. Refuse unexpected prepare work.
    frame: refuse_frame,
    command: refuse_command,
    streamline_loaded: no_streamline,
    load_library,
};

unsafe extern "C" fn setup_outputs(_: *const Setup, out: *mut Raw, chain: *mut Raw) -> i32 {
    unsafe {
        out.write(null_mut());
        chain.write(null_mut());
    }
    OK
}

unsafe extern "C" fn refuse_frame(_: *const Frame, _: Raw) -> i32 {
    INVALID
}

unsafe extern "C" fn refuse_command(_: Raw, _: Raw) -> i32 {
    INVALID
}

unsafe extern "C" fn no_streamline() -> u32 {
    0
}

unsafe extern "C" fn load_library(_: &Setup, _: *const u16) -> Raw {
    // The owner retires these real module references without resolving SDK exports.
    let kernel32 = rsf_game_sys::ffi_util::wide("kernel32.dll");
    unsafe { LoadLibraryW(kernel32.as_ptr()) }
}

/// The leaves the XeSS FG owner calls in the fixture-leaves archive.
pub(super) mod leaf {
    pub(in crate::d3d12_generation::xess) use super::{
        bind_xell, close, constants, create_chain, create_event, create_fence, drain, enabled,
        marker, max_count, present_status, resolve, sleep, sleep_mode, tag, version,
    };
    pub(in crate::d3d12_generation::xess) use super::{
        create_context as create_fg, create_context as create_xell, ok2 as destroy_fg,
        ok2 as destroy_xell, ok3 as compose, ok3 as count, ok3 as present_id,
    };
    use crate::d3d12_generation::Checks;

    pub(in crate::d3d12_generation::xess) fn checks() -> &'static Checks {
        &super::CHECKS
    }
}
pub(super) unsafe extern "C" fn resolve(_: &Setup, _: Raw, _: Raw, out: &mut Raw) -> i32 {
    *out = DEVICE.raw();
    OK
}

pub(super) unsafe extern "C" fn close(_: Raw) {}

pub(super) unsafe extern "C" fn create_context(_: Raw, _: Raw, out: *mut Raw) -> i32 {
    unsafe { out.write(DEVICE.raw()) };
    OK
}

pub(super) unsafe extern "C" fn ok2(_: Raw, _: Raw) -> i32 {
    OK
}

pub(super) unsafe extern "C" fn ok3(_: Raw, _: Raw, _: u32) -> i32 {
    OK
}

pub(super) unsafe extern "C" fn max_count(_: Raw, _: Raw, out: *mut u32) -> i32 {
    unsafe { out.write(2) };
    OK
}

pub(super) unsafe extern "C" fn bind_xell(_: Raw, _: Raw, _: Raw) -> i32 {
    OK
}

pub(super) unsafe extern "C" fn version(_: Raw, _: *mut Version) {}

pub(super) unsafe extern "C" fn create_chain(
    _: Raw,
    _: Raw,
    _: *const Setup,
    _: u32,
    out: *mut Raw,
) -> i32 {
    unsafe { out.write(CHAIN.retain()) };
    OK
}

pub(super) unsafe extern "C" fn tag(
    _: Raw,
    _: Raw,
    _: Raw,
    _: u32,
    _: u32,
    _: *const Resource,
) -> i32 {
    INVALID
}

pub(super) unsafe extern "C" fn constants(_: Raw, _: Raw, _: u32, _: *const Frame, _: u32) -> i32 {
    INVALID
}

pub(super) unsafe extern "C" fn present_status(_: Raw, _: Raw, _: *mut Present) -> i32 {
    OK
}

pub(super) unsafe extern "C" fn create_fence(_: Raw) -> Raw {
    FENCE.retain()
}

pub(super) unsafe extern "C" fn create_event() -> Raw {
    unsafe { CreateEventW(null_mut(), 0, 0, null_mut()) }
}

pub(super) unsafe extern "C" fn drain(queue: Raw, fence: Raw, event: Raw, _: u64) -> i32 {
    if queue == QUEUE.raw() && fence == FENCE.raw() && !event.is_null() {
        OK
    } else {
        INVALID
    }
}

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

pub(super) unsafe extern "C" fn sleep_mode(_: Raw, _: Raw, interval: u32, _: u32) -> i32 {
    let mut sdk = SDK.lock().unwrap();
    sdk.interval = interval;
    sdk.updates += 1;
    OK
}

pub(super) unsafe extern "C" fn sleep(_: Raw, _: Raw, id: u32) -> i32 {
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

pub(super) unsafe extern "C" fn marker(_: Raw, _: Raw, id: u32, kind: u32) -> i32 {
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

pub(super) unsafe extern "C" fn enabled(_: Raw, _: Raw, _: u32) -> i32 {
    if SDK.lock().unwrap().fail_enable {
        INVALID
    } else {
        OK
    }
}

struct Session(Raw);

// Only the owner's begin call is moved to the worker, and it joins before
// destruction. The production owner admits marker/configure concurrently.
unsafe impl Sync for Session {}

fn provider() -> &'static Provider {
    &PROVIDER
}

impl Session {
    fn create(&mut self) {
        // The mock setup callback accepts the zeroed header used by this fixture.
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.chain.hwnd = CHAIN.raw();
        setup.chain.d3d12_device = DEVICE.raw();
        setup.chain.d3d12_queue = QUEUE.raw();
        setup.chain.ui_mode = RSF_UI_MODE_NONE;
        setup.chain.max_generated_frames = 2;
        let mut present = null_mut();
        assert_eq!(
            unsafe { rsf_core_xeg12_create(&setup, &mut self.0, &mut present) },
            OK,
        );
        assert_eq!(present, CHAIN.raw());
    }

    fn begin(&self, source: u64) -> i32 {
        unsafe { (provider().begin_frame.unwrap())(self.0, source) }
    }

    fn configure(&self, frames: u32, interval: u32) -> i32 {
        let options = Options {
            struct_size: size_of::<Options>() as u32,
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
    std::panic::catch_unwind(check).unwrap_or(1)
}
