// SPDX-License-Identifier: MIT
use crate::sys::*;
use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{self, null_mut};
use std::sync::mpsc;
use std::time::Duration;
use windows_sys::Win32::System::{
    Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
        VirtualFree, VirtualProtect,
    },
    SystemInformation::{GetSystemInfo, SYSTEM_INFO},
};

#[path = "reservation_cases.rs"]
mod cases;

const SESSION: u64 = 41;
const RSF_GAME_CPU_SOURCE_EVENT_SIZE_1: u32 =
    std::mem::offset_of!(rsf_game_cpu_source_event, poll_begin_qpc) as u32;
type Event = rsf_game_cpu_event;
type Source = rsf_game_cpu_source_event;

enum Injection {
    Withdraw,
    ActualBegin,
}

#[derive(Default)]
struct Delivery {
    events: Vec<Event>,
    injection: Option<Injection>,
    failures: u32,
}

thread_local! {
    static DELIVERY: RefCell<Delivery> = const { RefCell::new(Delivery { events: Vec::new(), injection: None, failures: 0 }) };
}

fn delivery_clear() {
    DELIVERY.with_borrow_mut(|state| state.events.clear());
}

fn delivered_len() -> usize {
    DELIVERY.with_borrow(|state| state.events.len())
}

fn delivered_at(index: usize) -> Event {
    DELIVERY.with_borrow(|state| state.events[index])
}

fn delivered_last() -> Event {
    DELIVERY.with_borrow(|state| *state.events.last().unwrap())
}

fn inject(injection: Injection) {
    DELIVERY.with_borrow_mut(|state| state.injection = Some(injection));
}

fn source_stage_delivered() -> bool {
    DELIVERY.with_borrow(|state| {
        state
            .events
            .iter()
            .any(|e| e.stage >= RSF_GAME_CPU_SOURCE_RESERVE)
    })
}

unsafe extern "C" fn sink(_: *mut c_void, event: *const Event) {
    let event = unsafe { *event };
    // Source callbacks can read the committed ledger and dispatch again.
    let valid = if event.stage >= RSF_GAME_CPU_SOURCE_RESERVE {
        let value = read(event.source_frame_id);
        value.state != 0
            && (event.stage != RSF_GAME_CPU_SOURCE_RESERVE
                || value.state == RSF_NATIVE_CPU_RESERVATION_OPEN)
            && (event.stage != RSF_GAME_CPU_SOURCE_BIND
                || value.state == RSF_NATIVE_CPU_RESERVATION_BOUND)
    } else {
        true
    };
    let injection = DELIVERY.with_borrow_mut(|state| {
        state.failures += u32::from(!valid);
        state.events.push(event);
        if event.stage == RSF_GAME_CPU_SOURCE_BIND {
            state.injection.take()
        } else {
            None
        }
    });
    let accepted = match injection {
        Some(Injection::Withdraw) => {
            let mut abort = source_fact(
                event.source_frame_id,
                RSF_GAME_CPU_SOURCE_ABORT,
                event.timestamp_qpc + 1,
                0,
            );
            abort.header = Event {
                struct_size: size_of::<Source>() as u32,
                stage: RSF_GAME_CPU_SOURCE_ABORT,
                timestamp_qpc: event.timestamp_qpc + 1,
                ..event
            };
            abort.native_generation = 0;
            abort.reason = RSF_GAME_CPU_ABORT_QUIESCE;
            send(&abort)
        }
        Some(Injection::ActualBegin) => send(&Event {
            struct_size: size_of::<Event>() as u32,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            stage: RSF_GAME_CPU_SIMULATION_BEGIN,
            timestamp_qpc: event.timestamp_qpc + 1,
            flags: RSF_GAME_CPU_INPUT_KEYED,
            ..event
        }),
        None => true,
    };
    DELIVERY.with_borrow_mut(|state| state.failures += u32::from(!accepted));
}

fn fact(source: u64, stage: u32, stamp: u64, flags: u32) -> Event {
    Event {
        flags: RSF_GAME_CPU_INPUT_KEYED | flags,
        ..super::cpu_events::fact(SESSION, source, stage, stamp)
    }
}

fn source_fact(source: u64, stage: u32, stamp: u64, predecessor: u64) -> Source {
    Source {
        header: Event {
            struct_size: size_of::<Source>() as u32,
            render_expected: 0,
            event_version: RSF_GAME_CPU_EVENT_VERSION_SOURCE,
            flags: 0,
            ..fact(source, stage, stamp, 0)
        },
        predecessor_source_id: predecessor,
        native_generation: source * 10,
        reason: 0,
        reserved: 0,
        poll_begin_qpc: 0,
    }
}

trait Packet {}
impl Packet for Event {}
impl Packet for Source {}

fn send(event: &impl Packet) -> bool {
    unsafe { rsf_native_cpu_dispatch(ptr::from_ref(event).cast(), Some(sink), null_mut()) != 0 }
}

fn reserve(source: u64, stamp: u64, predecessor: u64) -> bool {
    let mut event = source_fact(source, RSF_GAME_CPU_SOURCE_RESERVE, stamp, predecessor);
    event.header.flags = RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
    send(&event)
}

fn sampled(source: u64, stamp: u64, produced: bool) -> bool {
    let mut event = source_fact(source, RSF_GAME_CPU_SOURCE_SAMPLED, stamp, 0);
    event.header.input_kind = RSF_GAME_INPUT_MOUSE;
    if produced {
        event.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
    }
    event.poll_begin_qpc = stamp - 1;
    send(&event)
}

fn bind(source: u64, stamp: u64) -> bool {
    send(&source_fact(source, RSF_GAME_CPU_SOURCE_BIND, stamp, 0))
}

fn abort_source(source: u64, stamp: u64, reason: u32) -> bool {
    let mut event = source_fact(source, RSF_GAME_CPU_SOURCE_ABORT, stamp, 0);
    event.reason = reason;
    send(&event)
}

fn present_result(session: u64, source: u64, outcome: u32, result: i32) -> bool {
    unsafe { rsf_native_cpu_present_result(session, source, outcome, result) != 0 }
}

fn presented(source: u64) -> bool {
    present_result(SESSION, source, RSF_NATIVE_CPU_PRESENT_RETURNED, 0)
}

fn read(source: u64) -> rsf_native_cpu_reservation {
    let mut value: rsf_native_cpu_reservation = unsafe { std::mem::zeroed() };
    value.struct_size = size_of_val(&value) as u32;
    if unsafe { rsf_native_cpu_reservation_read(SESSION, source, &mut value) } == 0 {
        value.state = 0;
    }
    value
}

fn counters() -> rsf_native_cpu_reservation_counters {
    let mut value: rsf_native_cpu_reservation_counters = unsafe { std::mem::zeroed() };
    value.struct_size = size_of_val(&value) as u32;
    unsafe { rsf_native_cpu_reservation_counters_get(&mut value) };
    value
}

fn read_frame(source: u64, output: &mut rsf_native_cpu_frame) -> bool {
    output.struct_size = size_of_val(output) as u32;
    unsafe { rsf_native_cpu_read(SESSION, source, output) != 0 }
}

fn read_counters(output: &mut rsf_native_cpu_counters) -> bool {
    output.struct_size = size_of_val(output) as u32;
    unsafe { rsf_native_cpu_counters_get(output) != 0 }
}

fn marker(source: u64, kind: u32) -> i32 {
    unsafe { rsf_native_cpu_marker(source, kind) }
}

struct Pages(*mut c_void);

impl Drop for Pages {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.0, 0, MEM_RELEASE) };
    }
}

fn short_packets_are_not_overread() {
    let mut system: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    unsafe { GetSystemInfo(&mut system) };
    let size = system.dwPageSize as usize;
    let pages = Pages(unsafe {
        VirtualAlloc(
            null_mut(),
            size * 2,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    });
    assert!(!pages.0.is_null());
    let guard = unsafe { pages.0.cast::<u8>().add(size) };
    let mut old = 0;
    assert_ne!(
        unsafe { VirtualProtect(guard.cast(), size, PAGE_NOACCESS, &mut old) },
        0
    );
    let edge = unsafe { guard.sub(size_of::<Event>()).cast::<Event>() };
    unsafe { edge.write(fact(1, RSF_GAME_CPU_FRAME_BEGIN, 1, 0)) };
    assert_ne!(
        unsafe { rsf_native_cpu_dispatch(edge, Some(sink), null_mut()) },
        0
    );
    let header = Event {
        struct_size: size_of::<Event>() as u32,
        ..source_fact(90, RSF_GAME_CPU_SOURCE_RESERVE, 2, 1).header
    };
    unsafe { edge.write(header) };
    assert_eq!(
        unsafe { rsf_native_cpu_dispatch(edge, Some(sink), null_mut()) },
        0
    );
    unsafe { rsf_native_cpu_release() };
}

struct Cancellation {
    entered: mpsc::Sender<()>,
    cancelled: mpsc::Receiver<()>,
    observed: bool,
}

unsafe extern "C" fn wait_for_cancel(user: *mut c_void, _: *const Event) {
    let value = unsafe { &mut *user.cast::<Cancellation>() };
    if value.entered.send(()).is_ok()
        && value.cancelled.recv_timeout(Duration::from_secs(2)).is_ok()
    {
        value.observed = read(2).state == RSF_NATIVE_CPU_RESERVATION_ABORTED;
    }
}

unsafe extern "C" fn ignore(_: *mut c_void, _: *const Event) {}

fn cancel_while_callback_waits() {
    assert!(send(&fact(1, RSF_GAME_CPU_FRAME_BEGIN, 10, 0)));
    let (entered, entered_wait) = mpsc::channel();
    let (cancel, cancelled) = mpsc::channel();
    std::thread::scope(|scope| {
        let waiting = scope.spawn(move || {
            let mut context = Cancellation {
                entered,
                cancelled,
                observed: false,
            };
            let mut event = source_fact(2, RSF_GAME_CPU_SOURCE_RESERVE, 20, 1);
            event.header.flags = RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
            let accepted = unsafe {
                rsf_native_cpu_dispatch(
                    ptr::from_ref(&event).cast(),
                    Some(wait_for_cancel),
                    (&raw mut context).cast(),
                )
            };
            (accepted, context.observed)
        });
        let entered = entered_wait.recv_timeout(Duration::from_secs(2)).is_ok();
        let mut abort = source_fact(2, RSF_GAME_CPU_SOURCE_ABORT, 21, 0);
        abort.reason = RSF_GAME_CPU_ABORT_QUIESCE;
        let aborted = unsafe {
            rsf_native_cpu_dispatch(ptr::from_ref(&abort).cast(), Some(ignore), null_mut())
        };
        let signalled = cancel.send(()).is_ok();
        assert_eq!(waiting.join().unwrap(), (1, true));
        assert!(entered && aborted == 1 && signalled);
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_cpu_reservation_fixture() -> i32 {
    unsafe { rsf_native_cpu_release() };
    let result = crate::run(|| {
        short_packets_are_not_overread();
        unsafe { rsf_native_cpu_rendered_source_set(1) };
        cases::run();
        unsafe { rsf_native_cpu_release() };
        cancel_while_callback_waits();
        assert_eq!(DELIVERY.with_borrow(|state| state.failures), 0);
        0
    });
    unsafe {
        rsf_native_cpu_rendered_source_set(0);
        rsf_native_cpu_release();
    }
    result
}
