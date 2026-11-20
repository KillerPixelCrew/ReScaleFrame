// SPDX-License-Identifier: MIT
use crate::native_cpu::rsf_native_cpu_ping_message;
use crate::sr_session::has_capacity;
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_INPUT_EVENT,
    RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_SAMPLE, RSF_GAME_CPU_INPUT_UNFRAMED,
    RSF_GAME_CPU_WINDOW_MESSAGE, rsf_game_cpu_event,
};
use std::mem::MaybeUninit;
use std::sync::{Mutex, atomic::AtomicU8};
use windows_sys::Win32::{
    Foundation::HWND,
    System::{
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        Threading::GetCurrentThreadId,
    },
    UI::WindowsAndMessaging::{
        MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage,
        PostThreadMessageW, WM_QUIT, WM_USER,
    },
};

const FEATURE_FAILED: i32 = -9;
const NOT_READY: i32 = -8;
const MAX_DEQUEUES: usize = 16;
const THREAD_MESSAGES: HWND = std::ptr::without_provenance_mut(usize::MAX);

// A private posted fence distinguishes queue registrations even on a reused
// thread. The cookie survives shutdown while any old messages can be queued.
static EPOCH_TAG: AtomicU8 = AtomicU8::new(0);

fn epoch_tag() -> isize {
    std::ptr::addr_of!(EPOCH_TAG) as isize
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Status {
    message: u32,
    available: u32,
    reports: u32,
}

struct Registration {
    thread: u32,
    status: Status,
    ready: bool,
}

struct Pcl {
    cookie: usize,
    registered: Option<Registration>,
}

static PCL: Mutex<Pcl> = Mutex::new(Pcl {
    cookie: 0,
    registered: None,
});

#[repr(C)]
struct Sdk {
    bind: unsafe extern "C" fn() -> i32,
    message: unsafe extern "C" fn() -> u32,
    reflex: unsafe extern "C" fn(*mut u32, *mut u32),
    report: unsafe extern "C" fn(i32),
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_pcl_reset() {
    if PCL.lock().unwrap().registered.take().is_some() {
        rsf_native_cpu_ping_message(0);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_pcl_status() -> Status {
    PCL.lock()
        .unwrap()
        .registered
        .as_ref()
        .map_or_else(Status::default, |r| r.status)
}

// The native caller serializes vendor registration against SDK initialization
// and shutdown. The PCL lock is never held for a vendor call.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_pcl_bind(use_input_thread: u32, sdk: &Sdk) -> i32 {
    let thread = unsafe { GetCurrentThreadId() };
    let previous = {
        let mut pcl = PCL.lock().unwrap();
        if pcl
            .registered
            .as_ref()
            .is_some_and(|r| use_input_thread == 0 || r.thread == thread)
        {
            return 1;
        }
        // The old queue stays untouched while the SDK changes its target.
        pcl.registered.take()
    };
    let mut message = MSG::default();
    unsafe { PeekMessageW(&mut message, THREAD_MESSAGES, WM_USER, WM_USER, PM_NOREMOVE) };
    let result = unsafe { (sdk.bind)() };
    unsafe { (sdk.report)(result) };
    if result != 0 {
        PCL.lock().unwrap().registered = previous;
        return 0;
    }
    let id = unsafe { (sdk.message)() };
    if !(0xc000..=0xffff).contains(&id) {
        if previous.is_some() {
            rsf_native_cpu_ping_message(0);
        }
        unsafe { (sdk.report)(NOT_READY) };
        return 0;
    }
    let mut status = Status {
        message: id,
        ..Status::default()
    };
    unsafe { (sdk.reflex)(&mut status.available, &mut status.reports) };
    let posted = {
        let mut pcl = PCL.lock().unwrap();
        pcl.cookie = pcl.cookie.wrapping_add(1).max(1);
        let posted = unsafe { PostThreadMessageW(thread, id, pcl.cookie, epoch_tag()) } != 0;
        pcl.registered = posted.then_some(Registration {
            thread,
            status,
            ready: false,
        });
        posted
    };
    if posted || previous.is_some() {
        rsf_native_cpu_ping_message(if posted { id } else { 0 });
    }
    if !posted {
        unsafe { (sdk.report)(FEATURE_FAILED) };
    }
    i32::from(posted)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_pcl_ping(window: HWND) -> i32 {
    let id = rsf_core_latencyvk_pcl_status().message;
    i32::from(!window.is_null() && id != 0 && unsafe { PostMessageW(window, id, 0, 0) } != 0)
}

// Reports only messages actually removed from the registered input thread.
// Keep WM_QUIT and unrelated messages in the engine's own queue.
fn collect(session: u64, delivered: &mut [MaybeUninit<rsf_game_cpu_event>; MAX_DEQUEUES]) -> usize {
    let mut pcl = PCL.lock().unwrap();
    let cookie = pcl.cookie;
    let Some(registration) = pcl.registered.as_mut() else {
        return 0;
    };
    if registration.thread != unsafe { GetCurrentThreadId() } {
        return 0;
    }
    let mut frequency = 0;
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        return 0;
    }

    let id = registration.status.message;
    let mut message = MSG::default();
    let mut count = 0;
    for _ in 0..MAX_DEQUEUES {
        // WM_QUIT bypasses PeekMessageW's range filter.
        if unsafe { PeekMessageW(&mut message, THREAD_MESSAGES, id, id, PM_NOREMOVE) } == 0
            || !message.hwnd.is_null()
            || message.message != id
        {
            break;
        }
        if unsafe { PeekMessageW(&mut message, THREAD_MESSAGES, id, id, PM_REMOVE) } == 0 {
            break;
        }
        if message.message == WM_QUIT {
            unsafe { PostQuitMessage(message.wParam as i32) };
            break;
        }
        if message.lParam == epoch_tag() {
            registration.ready |= message.wParam == cookie;
            continue;
        }
        if !registration.ready {
            continue;
        }
        let mut stamp = 0;
        if unsafe { QueryPerformanceCounter(&mut stamp) } == 0 || stamp <= 0 {
            continue;
        }
        delivered[count].write(rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            stage: RSF_GAME_CPU_INPUT_EVENT,
            session_id: session,
            source_frame_id: 0,
            timestamp_qpc: stamp as u64,
            qpc_frequency: frequency as u64,
            render_expected: 0,
            input_kind: 0,
            message_id: id,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            flags: RSF_GAME_CPU_WINDOW_MESSAGE | RSF_GAME_CPU_INPUT_UNFRAMED,
        });
        count += 1;
    }
    count
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_latencyvk_pcl_pump(
    boundary: *const rsf_game_cpu_event,
    dispatch: unsafe extern "C" fn(*const rsf_game_cpu_event),
) {
    if !unsafe { has_capacity(boundary) } {
        return;
    }
    let boundary = unsafe { &*boundary };
    if boundary.session_id == 0
        || boundary.source_frame_id == 0
        || boundary.event_version != RSF_GAME_CPU_EVENT_VERSION
        || boundary.flags & RSF_GAME_CPU_INPUT_KEYED == 0
        || !matches!(
            boundary.stage,
            RSF_GAME_CPU_FRAME_BEGIN | RSF_GAME_CPU_INPUT_SAMPLE
        )
    {
        return;
    }
    let mut delivered = [MaybeUninit::uninit(); MAX_DEQUEUES];
    let count = collect(boundary.session_id, &mut delivered);
    // Dispatch after the boundary and outside the queue lock. The sink may
    // reenter the SDK and the earlier boundary cannot consume a future ping.
    for fact in &delivered[..count] {
        unsafe { dispatch(fact.as_ptr()) };
    }
}
