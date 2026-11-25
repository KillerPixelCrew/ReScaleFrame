// SPDX-License-Identifier: MIT
//! The ctest native_fg_cpu_contract: the D3D CPU owner with recording leaves in place of the
//! presentation bridge.
use super::platform::rsf_native_fg_rendered_source_set;
use super::win32::{describe_address, duplicate_render_thread, sample_render_thread};
use super::*;
use crate::latency_lease::rsf_native_latency_has_claim;
use crate::native_cpu::{
    rsf_native_cpu_dispatch, rsf_native_cpu_present_result, rsf_native_cpu_read,
};
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::Duration;
use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::GetCurrentThreadId};

mod cases;

const SESSION: u64 = 41;
const PRIMARY_WINDOW: u64 = 51;
const REPLACEMENT_WINDOW: u64 = 52;
const XELL: u32 = RSF_PACING_XELL;

static GENERATION: AtomicU64 = AtomicU64::new(11);
static PACE_CALLS: AtomicU32 = AtomicU32::new(0);
static ABORT_CALLS: AtomicU32 = AtomicU32::new(0);
static RECEIPT_CALLS: AtomicU32 = AtomicU32::new(0);
static RETIRE_CALLS: AtomicU32 = AtomicU32::new(0);
static FAILURES: AtomicU32 = AtomicU32::new(0);
static REBIND: AtomicBool = AtomicBool::new(false);
static FAIL_MARKER: AtomicBool = AtomicBool::new(false);
static RETIRE_HOST: AtomicBool = AtomicBool::new(false);
// A full Streamline token ring: acquire refuses until a host abort frees a token.
static TOKENS_FULL: AtomicBool = AtomicBool::new(false);
static HOST_ABORTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

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
    Event {
        render_expected: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: RSF_GAME_CPU_INPUT_KEYED,
        ..rsf_test_support::cpu_event(
            SESSION,
            source_frame_id,
            stage,
            rsf_test_support::qpc(),
            rsf_test_support::qpc_frequency(),
        )
    }
}

unsafe extern "C" fn sink(_: *mut c_void, event: *const Event) {
    unsafe { fg_cpu(event) };
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
    unsafe { latency_window_bind(&binding) }
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
    assert_ne!(unsafe { latency_frame_get(&mut out) }, 0);
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

pub(super) extern "C" fn has_owner() -> i32 {
    1
}
pub(super) extern "C" fn generation() -> u64 {
    GENERATION.load(Relaxed)
}
pub(super) extern "C" fn backend() -> u32 {
    RSF_FG_BACKEND_XESS
}
pub(super) extern "C" fn device() -> *mut c_void {
    // Any stable address stands in for the D3D12 device.
    ptr::from_ref(&GENERATION).cast_mut().cast()
}
pub(super) extern "C" fn vendor(_: *mut c_void) -> u32 {
    0x8086
}
pub(super) extern "C" fn host() -> *mut c_void {
    if RETIRE_HOST.load(Relaxed) {
        device()
    } else {
        null_mut()
    }
}
pub(super) extern "C" fn host_retire(_: *mut c_void, _: u64) -> i32 {
    RETIRE_CALLS.fetch_add(1, Relaxed);
    0
}
pub(super) unsafe extern "C" fn status(out: *mut rsf_d3d11_present_status) -> i32 {
    let out = unsafe { &mut *out };
    out.backend = RSF_FG_BACKEND_XESS;
    out.state.pacing_owner = XELL;
    out.state.low_latency_available = 1;
    0
}
pub(super) extern "C" fn acquire(_: u64) -> i32 {
    if TOKENS_FULL.load(Relaxed) {
        NOT_READY
    } else {
        0
    }
}
// The fixture host is a stand-in address: its token ring is TOKENS_FULL and it reports no
// latency status.
pub(super) unsafe fn host_abort(host: *mut c_void, source: u64) -> i32 {
    if host.is_null() {
        return INVALID;
    }
    HOST_ABORTS.lock().unwrap().push(source);
    TOKENS_FULL.store(false, Relaxed);
    0
}
pub(super) unsafe fn host_status(_: *mut c_void, _: *mut rsf_streamline_latency_status) -> i32 {
    NOT_READY
}
pub(super) extern "C" fn pace(source: u64, owner: u32, expected: u64) -> i32 {
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
            return NOT_READY;
        }
    }
    0
}
pub(super) extern "C" fn marker(expected: u64, source: u64, kind: u32, controller: u32) -> i32 {
    if FAIL_MARKER.load(Relaxed) {
        verify(kind == SIM_START);
        return -9;
    }
    verify(expected == generation() && controller == 0);
    STATE.lock().unwrap().markers.push((source, kind));
    if source == 1 && kind == PRESENT_END {
        wake();
    }
    0
}
pub(super) extern "C" fn route_receipt(expected: u64, source: u64, kind: u32, message: u32) -> i32 {
    verify(expected == generation() && source == 2 && kind == RSF_GAME_INPUT_MOUSE && message == 0);
    RECEIPT_CALLS.fetch_add(1, Relaxed);
    if REBIND.load(Relaxed) {
        verify(bind(REPLACEMENT_WINDOW) == 0);
    }
    0
}
pub(super) extern "C" fn abort(_: u64) -> i32 {
    ABORT_CALLS.fetch_add(1, Relaxed);
    0
}
pub(super) extern "C" fn wait_completion(timeout_ms: u32, pump_sent_messages: u32) -> i32 {
    if pump_sent_messages == 0 {
        STATE.lock().unwrap().waiting_for_predecessor = true;
        CHANGED.notify_all();
    }
    std::thread::sleep(Duration::from_millis(u64::from(timeout_ms)));
    1
}
pub(super) extern "C" fn owned_window_thread(key: u64) -> u32 {
    if matches!(key, PRIMARY_WINDOW | REPLACEMENT_WINDOW) {
        unsafe { GetCurrentThreadId() }
    } else {
        0
    }
}

/// The leaves the CPU owner calls in the fixture-leaves archive: recording doubles for the
/// presentation bridge, and the production Streamline host and Anti-Lag leaves.
pub(super) mod leaf {
    pub(in crate::native_fg_cpu) use super::super::platform::leaf::{
        allows_fg, create_antilag, host_presented, host_reflex, host_reset, host_token, input,
    };
    pub(in crate::native_fg_cpu) use super::{
        abort, acquire, backend, device, generation, has_owner, host, host_abort, host_retire,
        host_status, marker, pace, route_receipt as receipt, status, vendor as adapter_vendor,
    };

    pub(in crate::native_fg_cpu) const FIXTURE_OWNED_WINDOW_THREAD: Option<
        unsafe extern "C" fn(u64) -> u32,
    > = Some(super::owned_window_thread);
    pub(in crate::native_fg_cpu) const FIXTURE_WAIT_COMPLETION: Option<
        unsafe extern "C" fn(u32, u32) -> i32,
    > = Some(super::wait_completion);
}
fn sample_native_thread() {
    let (duplicate, handle) = mpsc::channel();
    let (release, done) = mpsc::channel();
    std::thread::scope(|scope| {
        let renderer = scope.spawn(move || {
            let thread = duplicate_render_thread() as usize;
            let _ = duplicate.send(thread);
            let _ = done.recv_timeout(Duration::from_secs(3));
        });
        let thread = handle.recv_timeout(Duration::from_secs(2)).unwrap() as *mut c_void;
        assert!(!thread.is_null());
        let mut addresses = [0; 9];
        let count =
            unsafe { sample_render_thread(thread, addresses.as_mut_ptr(), addresses.len()) };
        let mut location = [0; 384];
        let length =
            unsafe { describe_address(addresses[0], location.as_mut_ptr(), location.len()) };
        let _ = release.send(());
        renderer.join().unwrap();
        unsafe { CloseHandle(thread) };
        assert!((1..=9).contains(&count) && addresses[0] != 0 && length > 0);
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_fg_cpu_fixture() -> i32 {
    std::panic::catch_unwind(cases::run).unwrap_or(1)
}
