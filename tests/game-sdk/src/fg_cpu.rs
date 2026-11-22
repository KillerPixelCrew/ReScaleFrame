// SPDX-License-Identifier: MIT
use crate::{cpu_abi::*, cpu_calls::*, fg_cpu_calls::*, native_fg_cpu_abi::*};
use rsf_game_sys::*;
use std::ffi::c_void;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Condvar, Mutex, OnceLock, mpsc};
use std::time::Duration;
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        Threading::GetCurrentThreadId,
    },
};

#[path = "fg_cpu_cases.rs"]
mod cases;

const SESSION: u64 = 41;
const PRIMARY_WINDOW: u64 = 51;
const REPLACEMENT_WINDOW: u64 = 52;
const XELL: u32 = crate::render_abi::RSF_PACING_XELL;
type Event = rsf_game_cpu_event;
type Platform = rsf_nfg_cpu_platform;

static GENERATION: AtomicU64 = AtomicU64::new(11);
static PACE_CALLS: AtomicU32 = AtomicU32::new(0);
static ABORT_CALLS: AtomicU32 = AtomicU32::new(0);
static RECEIPT_CALLS: AtomicU32 = AtomicU32::new(0);
static RETIRE_CALLS: AtomicU32 = AtomicU32::new(0);
static FAILURES: AtomicU32 = AtomicU32::new(0);
static REBIND: AtomicBool = AtomicBool::new(false);
static FAIL_MARKER: AtomicBool = AtomicBool::new(false);
static RETIRE_HOST: AtomicBool = AtomicBool::new(false);

struct State {
    blocked_source: u64,
    sleeping: bool,
    may_wake: bool,
    waiting_for_predecessor: bool,
    markers: Vec<(u64, u32)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    blocked_source: 0,
    sleeping: false,
    may_wake: false,
    waiting_for_predecessor: false,
    markers: Vec::new(),
});
static CHANGED: Condvar = Condvar::new();
static PLATFORM: OnceLock<Platform> = OnceLock::new();

fn verify(value: bool) {
    FAILURES.fetch_add(u32::from(!value), Relaxed);
}

fn pace_calls() -> u32 {
    PACE_CALLS.load(Relaxed)
}
fn abort_calls() -> u32 {
    ABORT_CALLS.load(Relaxed)
}
fn receipt_calls() -> u32 {
    RECEIPT_CALLS.load(Relaxed)
}
fn marker_count() -> usize {
    STATE.lock().unwrap().markers.len()
}

fn fact(source_frame_id: u64, stage: u32) -> Event {
    let (mut stamp, mut frequency) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut stamp);
        QueryPerformanceFrequency(&mut frequency);
    }
    Event {
        struct_size: size_of::<Event>() as u32,
        stage,
        session_id: SESSION,
        source_frame_id,
        timestamp_qpc: stamp as u64,
        qpc_frequency: frequency as u64,
        render_expected: 0,
        input_kind: 0,
        message_id: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: RSF_GAME_CPU_INPUT_KEYED,
    }
}

unsafe extern "C" fn sink(_: *mut c_void, event: *const Event) {
    unsafe { rsf_core_native_fg_cpu(event, platform()) };
}

fn dispatch(event: *const Event) -> bool {
    unsafe { rsf_native_cpu_dispatch(event, Some(sink), null_mut()) != 0 }
}

fn send(event: Event) -> bool {
    dispatch(&event)
}

fn source_event(source: u64, stage: u32, predecessor_source_id: u64) -> bool {
    let event = rsf_game_cpu_source_event {
        header: Event {
            struct_size: size_of::<rsf_game_cpu_source_event>() as u32,
            event_version: RSF_GAME_CPU_EVENT_VERSION_SOURCE,
            flags: if predecessor_source_id == 0 {
                0
            } else {
                RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR
            },
            ..fact(source, stage)
        },
        native_generation: 1,
        predecessor_source_id,
        reserved: 0,
        poll_begin_qpc: 0,
        reason: if stage == RSF_GAME_CPU_SOURCE_ABORT {
            RSF_GAME_CPU_ABORT_QUIESCE
        } else {
            0
        },
    };
    dispatch(ptr::from_ref(&event).cast())
}

fn bind(window_key: u64) -> i32 {
    let binding = rsf_native_latency_window {
        struct_size: size_of::<rsf_native_latency_window>() as u32,
        version: 1,
        session_id: SESSION,
        window_key,
    };
    unsafe { rsf_core_native_latency_window_bind(&binding, platform()) }
}

fn receipt(receipt_id: u64, window_key: u64) -> bool {
    let event = rsf_game_cpu_receipt_event {
        header: Event {
            struct_size: size_of::<rsf_game_cpu_receipt_event>() as u32,
            event_version: RSF_GAME_CPU_EVENT_VERSION_RECEIPT,
            flags: RSF_GAME_CPU_INPUT_UNFRAMED,
            input_kind: RSF_GAME_INPUT_MOUSE,
            ..fact(0, RSF_GAME_CPU_INPUT_EVENT)
        },
        receipt_id,
        window_key,
    };
    dispatch(ptr::from_ref(&event).cast())
}

fn arm(source: u64) {
    let mut state = STATE.lock().unwrap();
    state.blocked_source = source;
    state.sleeping = false;
    state.may_wake = false;
}

fn wait_state(ready: impl Fn(&State) -> bool) -> bool {
    let (state, _) = CHANGED
        .wait_timeout_while(STATE.lock().unwrap(), Duration::from_secs(2), |state| {
            !ready(state)
        })
        .unwrap();
    ready(&state)
}

fn wait_for_sleep() -> bool {
    wait_state(|state| state.sleeping)
}

fn wake() {
    STATE.lock().unwrap().may_wake = true;
    CHANGED.notify_all();
}

fn request_frame(source: u64) -> rsf_native_latency_frame {
    rsf_native_latency_frame {
        struct_size: size_of::<rsf_native_latency_frame>() as u32,
        version: 1,
        source,
        ..Default::default()
    }
}

fn snapshot(source: u64) -> rsf_native_latency_frame {
    let mut out = request_frame(source);
    assert_ne!(unsafe { rsf_core_native_latency_frame_get(&mut out) }, 0);
    out
}

fn routes() -> rsf_native_input_routes {
    let mut out = rsf_native_input_routes {
        struct_size: size_of::<rsf_native_input_routes>() as u32,
        version: 1,
        ..Default::default()
    };
    assert_ne!(unsafe { rsf_core_native_input_routes_get(&mut out) }, 0);
    out
}

extern "C" fn has_owner() -> i32 {
    1
}
extern "C" fn generation() -> u64 {
    GENERATION.load(Relaxed)
}
extern "C" fn backend() -> u32 {
    RSF_FG_BACKEND_XESS
}
extern "C" fn device() -> *mut c_void {
    ptr::from_ref(platform()).cast_mut().cast()
}
extern "C" fn vendor(_: *mut c_void) -> u32 {
    0x8086
}
extern "C" fn host() -> *mut c_void {
    if RETIRE_HOST.load(Relaxed) {
        device()
    } else {
        null_mut()
    }
}
extern "C" fn host_retire(_: *mut c_void, _: u64) -> i32 {
    RETIRE_CALLS.fetch_add(1, Relaxed);
    0
}
unsafe extern "C" fn status(out: *mut rsf_d3d11_present_status) -> i32 {
    let out = unsafe { &mut *out };
    out.backend = RSF_FG_BACKEND_XESS;
    out.state.pacing_owner = XELL;
    out.state.low_latency_available = 1;
    0
}
extern "C" fn acquire(_: u64) -> i32 {
    0
}
extern "C" fn pace(source: u64, owner: u32, expected: u64) -> i32 {
    verify(owner == XELL && expected == generation());
    PACE_CALLS.fetch_add(1, Relaxed);
    let mut state = STATE.lock().unwrap();
    if source == state.blocked_source {
        state.sleeping = true;
        CHANGED.notify_all();
        let (state, _) = CHANGED
            .wait_timeout_while(state, Duration::from_secs(3), |state| !state.may_wake)
            .unwrap();
        if !state.may_wake {
            return RSF_NFG_CPU_NOT_READY;
        }
    }
    0
}
extern "C" fn marker(expected: u64, source: u64, kind: u32, controller: u32) -> i32 {
    if FAIL_MARKER.load(Relaxed) {
        verify(kind == RSF_NFG_CPU_SIM_START);
        return -9;
    }
    verify(expected == generation() && controller == 0);
    STATE.lock().unwrap().markers.push((source, kind));
    if source == 1 && kind == RSF_NFG_CPU_PRESENT_END {
        wake();
    }
    0
}
extern "C" fn route_receipt(expected: u64, source: u64, kind: u32, message: u32) -> i32 {
    verify(expected == generation() && source == 2 && kind == RSF_GAME_INPUT_MOUSE && message == 0);
    RECEIPT_CALLS.fetch_add(1, Relaxed);
    if REBIND.load(Relaxed) {
        verify(bind(REPLACEMENT_WINDOW) == 0);
    }
    0
}
extern "C" fn abort(_: u64) -> i32 {
    ABORT_CALLS.fetch_add(1, Relaxed);
    0
}
extern "C" fn wait_completion(timeout_ms: u32, pump_sent_messages: u32) -> i32 {
    if pump_sent_messages == 0 {
        STATE.lock().unwrap().waiting_for_predecessor = true;
        CHANGED.notify_all();
    }
    std::thread::sleep(Duration::from_millis(u64::from(timeout_ms)));
    1
}
extern "C" fn owned_window_thread(key: u64) -> u32 {
    if matches!(key, PRIMARY_WINDOW | REPLACEMENT_WINDOW) {
        unsafe { GetCurrentThreadId() }
    } else {
        0
    }
}

fn platform() -> &'static Platform {
    PLATFORM.get_or_init(|| Platform {
        has_owner: Some(has_owner),
        generation: Some(generation),
        backend: Some(backend),
        device: Some(device),
        adapter_vendor: Some(vendor),
        host: Some(host),
        host_retire: Some(host_retire),
        status: Some(status),
        acquire: Some(acquire),
        pace: Some(pace),
        marker: Some(marker),
        receipt: Some(route_receipt),
        abort: Some(abort),
        fixture_wait_completion: Some(wait_completion),
        fixture_owned_window_thread: Some(owned_window_thread),
        ..unsafe { rsf_native_fg_cpu_platform }
    })
}

fn sample_native_thread() {
    let (duplicate, handle) = mpsc::channel();
    let (release, done) = mpsc::channel();
    std::thread::scope(|scope| {
        let renderer = scope.spawn(move || {
            let thread = unsafe { rsf_core_nfg_duplicate_render_thread() } as usize;
            let _ = duplicate.send(thread);
            let _ = done.recv_timeout(Duration::from_secs(3));
        });
        let thread = handle.recv_timeout(Duration::from_secs(2)).unwrap() as *mut c_void;
        assert!(!thread.is_null());
        let mut addresses = [0; 9];
        let count = unsafe {
            rsf_core_nfg_sample_render_thread(thread, addresses.as_mut_ptr(), addresses.len())
        };
        let mut location = [0; 384];
        let length = unsafe {
            rsf_core_nfg_describe_address(addresses[0], location.as_mut_ptr(), location.len())
        };
        let _ = release.send(());
        renderer.join().unwrap();
        unsafe { CloseHandle(thread) };
        assert!((1..=9).contains(&count) && addresses[0] != 0 && length > 0);
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_fg_cpu_fixture() -> i32 {
    crate::run(cases::run)
}
