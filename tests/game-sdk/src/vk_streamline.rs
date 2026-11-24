// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::backend_result::{FEATURE_FAILED, INVALID_ARGUMENT, OK};
use rsf_test_support::{qpc, qpc_frequency};
use std::cell::Cell;
use std::sync::atomic::{AtomicI32, AtomicUsize};
use windows_sys::Win32::{System::Threading::GetCurrentThreadId, UI::WindowsAndMessaging::*};

#[path = "vk_streamline_cases.rs"]
mod cases;

const PCL_MESSAGE: u32 = 0xc123;
const THREAD_MESSAGES: *mut c_void = ptr::without_provenance_mut(usize::MAX);
const BEGIN: u32 = RSF_GAME_CPU_FRAME_BEGIN;
const SAMPLE: u32 = RSF_GAME_CPU_INPUT_SAMPLE;
const SIM_BEGIN: u32 = RSF_GAME_CPU_SIMULATION_BEGIN;
const SIM_END: u32 = RSF_GAME_CPU_SIMULATION_END;
const END: u32 = RSF_GAME_CPU_FRAME_END;
const PACE: u32 = RSF_GAME_CPU_PACING;
const INPUT: u32 = RSF_GAME_CPU_INPUT_EVENT;
const KEYED: u32 = RSF_GAME_CPU_INPUT_KEYED;
const V1: u32 = RSF_GAME_CPU_EVENT_VERSION;

#[derive(Clone, Copy)]
struct Marker {
    frame: u32,
    kind: u32,
}

struct Tokens {
    cursor: usize,
    slots: [u32; 6],
}

static TOKENS: Mutex<Tokens> = Mutex::new(Tokens {
    cursor: 0,
    slots: [0; 6],
});
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static FAIL_BEGIN: AtomicBool = AtomicBool::new(false);
static SLEPT: AtomicU32 = AtomicU32::new(0);
static OPTIONS: AtomicU32 = AtomicU32::new(0);
static LOW_LATENCY: AtomicU32 = AtomicU32::new(1);
static SHUTDOWN_ALLOWED: AtomicBool = AtomicBool::new(true);
static PCL_BIND_FAIL: AtomicBool = AtomicBool::new(false);
static PCL_BINDS: AtomicU32 = AtomicU32::new(0);
static PCL_THREAD: AtomicU32 = AtomicU32::new(0);
static SR: AtomicU32 = AtomicU32::new(0);
static FG: AtomicU32 = AtomicU32::new(0);
static LEASED: AtomicU32 = AtomicU32::new(0);
static MARKERS: Mutex<Vec<Marker>> = Mutex::new(Vec::new());
static XELL_MARKERS: Mutex<Vec<Marker>> = Mutex::new(Vec::new());
static XELL_RESULT: AtomicI32 = AtomicI32::new(0);
static XELL_SLEEPS: AtomicU32 = AtomicU32::new(0);
static XELL_MODE: AtomicU32 = AtomicU32::new(0);
static XELL_SLEEP: Gate = Gate::new();
static REFLEX_SLEEP: Gate = Gate::new();

thread_local! { static CHECK_REGISTRATION: Cell<bool> = const { Cell::new(false) }; }

fn cpu(frame: u64, stage: u32) {
    cpu_expected(frame, stage, 1);
}
fn cpu_expected(frame: u64, stage: u32, render_expected: u32) {
    let event = Event {
        render_expected,
        ..event(1, frame, stage, 0, 0, 0, 0)
    };
    unsafe { rsf_latencyvk_cpu(&event) };
}
fn clock_event(frame: u64, stage: u32) -> Event {
    event(909, frame, stage, qpc(), qpc_frequency(), V1, KEYED)
}
fn report_with_registration(event: &Event) {
    CHECK_REGISTRATION.set(true);
    unsafe { rsf_latencyvk_cpu(event) };
    CHECK_REGISTRATION.set(false);
}
fn check_registration() {
    if CHECK_REGISTRATION.get() {
        verify(PCL_THREAD.load(Relaxed) == unsafe { GetCurrentThreadId() });
    }
}
fn marker_count() -> usize {
    MARKERS.lock().unwrap().len()
}
fn markers() -> Vec<Marker> {
    MARKERS.lock().unwrap().clone()
}
fn xell_marker_count() -> usize {
    XELL_MARKERS.lock().unwrap().len()
}
fn xell_markers() -> Vec<Marker> {
    XELL_MARKERS.lock().unwrap().clone()
}
fn token_index(token: usize) -> Option<u32> {
    let index = token.checked_sub(1)?;
    TOKENS.lock().unwrap().slots.get(index).copied()
}

pub(super) fn xell_options(mode: u32) -> i32 {
    XELL_MODE.store(mode, Relaxed);
    OK
}
pub(super) fn xell_sleep(context: *mut rsf_fgvk) -> i32 {
    verify(!context.is_null());
    XELL_SLEEPS.fetch_add(1, Relaxed);
    if verify(XELL_SLEEP.pause()) {
        OK
    } else {
        INVALID_ARGUMENT
    }
}
pub(super) fn xell_marker(frame: u64, kind: u32) -> i32 {
    let result = XELL_RESULT.load(Relaxed);
    if result == OK {
        XELL_MARKERS.lock().unwrap().push(Marker {
            frame: frame as u32,
            kind,
        });
    }
    result
}
pub(super) fn xell_status() -> rsf_fgvk_latency_status {
    rsf_fgvk_latency_status {
        struct_size: size_of::<rsf_fgvk_latency_status>() as u32,
        abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
        mode: XELL_MODE.load(Relaxed),
        available: 1,
        sleep_calls: u64::from(XELL_SLEEPS.load(Relaxed)),
        marker_calls: xell_marker_count() as u64,
        result: 0,
    }
}

// Synthetic Streamline bridge calls for the latency owner (SDK below). The token handles
// address six mock slots and never leave this fixture executable.
extern "C" fn sdk_initialize_features(
    _: *const u16,
    _: *const c_char,
    _: u32,
    fg: u32,
    sr: u32,
) -> i32 {
    SR.store(sr, Relaxed);
    FG.store(fg, Relaxed);
    OK
}
extern "C" fn sdk_shutdown_checked() -> i32 {
    if SHUTDOWN_ALLOWED.load(Relaxed) {
        OK
    } else {
        INVALID_ARGUMENT
    }
}
extern "C" fn sdk_module() -> *mut c_void {
    identity(1)
}
extern "C" fn sdk_bind_reflex() -> i32 {
    OK
}
extern "C" fn sdk_bind_pcl() -> i32 {
    PCL_BINDS.fetch_add(1, Relaxed);
    if PCL_BIND_FAIL.load(Relaxed) {
        return -17;
    }
    PCL_THREAD.store(unsafe { GetCurrentThreadId() }, Relaxed);
    OK
}
extern "C" fn sdk_pcl_window_message() -> u32 {
    PCL_MESSAGE
}
unsafe extern "C" fn sdk_get_reflex_state(available: *mut u32, reports: *mut u32) -> i32 {
    unsafe {
        available.write(LOW_LATENCY.load(Relaxed));
        reports.write(1);
    }
    OK
}
extern "C" fn sdk_set_reflex_interval(mode: i32, interval: u32) -> i32 {
    if !verify(
        (mode == 1 && interval == 12345)
            || (XELL_MODE.load(Relaxed) == 1 && mode == 0 && interval == 0),
    ) {
        return INVALID_ARGUMENT;
    }
    OPTIONS.fetch_add(1, Relaxed);
    OK
}
unsafe extern "C" fn sdk_begin_frame_checked(frame: u32, token: *mut usize) -> i32 {
    let mut tokens = TOKENS.lock().unwrap();
    let cursor = tokens.cursor;
    tokens.slots[cursor] = frame;
    let next = cursor + 1;
    tokens.cursor = next % tokens.slots.len();
    unsafe { token.write(next) };
    if FAIL_BEGIN.load(Relaxed) {
        return -19;
    }
    CURRENT.store(next, Relaxed);
    OK
}
extern "C" fn sdk_current_frame_token() -> usize {
    CURRENT.load(Relaxed)
}
extern "C" fn sdk_token_matches(token: usize, index: u32) -> i32 {
    i32::from(token_index(token) == Some(index))
}
extern "C" fn sdk_reflex_sleep() -> i32 {
    check_registration();
    if !verify(REFLEX_SLEEP.pause()) {
        return INVALID_ARGUMENT;
    }
    SLEPT.fetch_add(1, Relaxed);
    OK
}
extern "C" fn sdk_marker_for_token(token: usize, kind: u32) -> i32 {
    check_registration();
    let Some(frame) = token_index(token) else {
        verify(false);
        return INVALID_ARGUMENT;
    };
    MARKERS.lock().unwrap().push(Marker { frame, kind });
    OK
}
unsafe extern "C" fn sdk_without_dlss_g(callback: rsf_vk_wsi_call, user: *mut c_void) -> i32 {
    unsafe { callback.unwrap()(user) }
}

pub(super) fn run() -> i32 {
    cases::run();
    assert_eq!(CALLBACK_FAILURES.load(Relaxed), 0);
    0
}

/// A borrowed callback runs directly; the fixture's callbacks never throw.
unsafe extern "C" fn sdk_invoke(
    callback: rsf_slvk_callback,
    module: *mut c_void,
    token: usize,
    user: *mut c_void,
) -> i32 {
    callback.map_or(FEATURE_FAILED, |callback| unsafe {
        callback(module, token, user)
    })
}

/// The fake Streamline the latency owner calls in place of rsf-streamline-vk.
pub(super) static SDK: Streamline = Streamline {
    initialize: Some(sdk_initialize_features),
    shutdown: Some(sdk_shutdown_checked),
    module: Some(sdk_module),
    invoke: Some(sdk_invoke),
    set_interval: Some(sdk_set_reflex_interval),
    sleep: Some(sdk_reflex_sleep),
    tokens: TokenCalls {
        begin: Some(sdk_begin_frame_checked),
        current: Some(sdk_current_frame_token),
        matches: Some(sdk_token_matches),
        mark: Some(sdk_marker_for_token),
    },
    pcl: PclCalls {
        bind: Some(sdk_bind_pcl),
        message: Some(sdk_pcl_window_message),
        bind_reflex: Some(sdk_bind_reflex),
        reflex: Some(sdk_get_reflex_state),
    },
};
pub(super) const WITHOUT_DLSS_G: WsiScope = Some(sdk_without_dlss_g);
