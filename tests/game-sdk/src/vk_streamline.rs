// SPDX-License-Identifier: MIT
use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicI32, AtomicUsize};
use windows_sys::Win32::{
    System::{
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        Threading::GetCurrentThreadId,
    },
    UI::WindowsAndMessaging::*,
};

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
    let (mut now, mut hz) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut hz);
    }
    event(909, frame, stage, now as u64, hz as u64, V1, KEYED)
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
        INVALID
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

// Synthetic Streamline SDK exports for the native consumer. The token handles
// address six mock slots and never leave this fixture executable.
#[unsafe(export_name = "rsf_vk_sl_initialize_features")]
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
#[unsafe(export_name = "rsf_vk_sl_shutdown_checked")]
extern "C" fn sdk_shutdown_checked() -> i32 {
    if SHUTDOWN_ALLOWED.load(Relaxed) {
        OK
    } else {
        INVALID
    }
}
#[unsafe(export_name = "rsf_vk_sl_module")]
extern "C" fn sdk_module() -> *mut c_void {
    identity(1)
}
#[unsafe(export_name = "rsf_vk_sl_get_instance_proc_addr")]
extern "C" fn sdk_get_instance_proc_addr(_: *mut c_void, _: *const c_char) -> rsf_vk_function {
    None
}
#[unsafe(export_name = "rsf_vk_sl_bind_reflex")]
extern "C" fn sdk_bind_reflex() -> i32 {
    OK
}
#[unsafe(export_name = "rsf_vk_sl_bind_pcl")]
extern "C" fn sdk_bind_pcl() -> i32 {
    PCL_BINDS.fetch_add(1, Relaxed);
    if PCL_BIND_FAIL.load(Relaxed) {
        return -17;
    }
    PCL_THREAD.store(unsafe { GetCurrentThreadId() }, Relaxed);
    OK
}
#[unsafe(export_name = "rsf_vk_sl_pcl_window_message")]
extern "C" fn sdk_pcl_window_message() -> u32 {
    PCL_MESSAGE
}
#[unsafe(export_name = "rsf_vk_sl_get_reflex_state")]
unsafe extern "C" fn sdk_get_reflex_state(available: *mut u32, reports: *mut u32) -> i32 {
    unsafe {
        available.write(LOW_LATENCY.load(Relaxed));
        reports.write(1);
    }
    OK
}
#[unsafe(export_name = "rsf_vk_sl_set_reflex_interval")]
extern "C" fn sdk_set_reflex_interval(mode: i32, interval: u32) -> i32 {
    if !verify(
        (mode == 1 && interval == 12345)
            || (XELL_MODE.load(Relaxed) == 1 && mode == 0 && interval == 0),
    ) {
        return INVALID;
    }
    OPTIONS.fetch_add(1, Relaxed);
    OK
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_begin_frame_checked(frame: u32, token: *mut usize) -> i32 {
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
#[unsafe(export_name = "rsf_vk_sl_current_frame_token")]
extern "C" fn sdk_current_frame_token() -> usize {
    CURRENT.load(Relaxed)
}
#[unsafe(export_name = "rsf_vk_sl_token_matches")]
extern "C" fn sdk_token_matches(token: usize, index: u32) -> i32 {
    i32::from(token_index(token) == Some(index))
}
#[unsafe(export_name = "rsf_vk_sl_reflex_sleep")]
extern "C" fn sdk_reflex_sleep() -> i32 {
    check_registration();
    if !verify(REFLEX_SLEEP.pause()) {
        return INVALID;
    }
    SLEPT.fetch_add(1, Relaxed);
    OK
}
#[unsafe(export_name = "rsf_vk_sl_marker_for_token")]
extern "C" fn sdk_marker_for_token(token: usize, kind: u32) -> i32 {
    check_registration();
    let Some(frame) = token_index(token) else {
        verify(false);
        return INVALID;
    };
    MARKERS.lock().unwrap().push(Marker { frame, kind });
    OK
}
#[unsafe(export_name = "rsf_vk_sl_queue_present_synchronized")]
extern "C" fn sdk_queue_present_synchronized(_: usize, _: *mut c_void, _: *const c_void) -> i32 {
    OK
}
#[unsafe(export_name = "rsf_vk_sl_vulkan_wsi_call")]
unsafe extern "C" fn sdk_vulkan_wsi_call(callback: rsf_vk_wsi_call, user: *mut c_void) -> i32 {
    unsafe { callback.unwrap()(user) }
}
#[unsafe(export_name = "rsf_vk_sl_without_dlss_g")]
unsafe extern "C" fn sdk_without_dlss_g(callback: rsf_vk_wsi_call, user: *mut c_void) -> i32 {
    unsafe { callback.unwrap()(user) }
}

pub(super) fn run() -> i32 {
    cases::run();
    assert_eq!(CALLBACK_FAILURES.load(Relaxed), 0);
    0
}
