// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_test_support::cpu_event;
use std::ffi::c_void;

type Event = rsf_game_cpu_event;
const BEGIN: u32 = RSF_GAME_CPU_FRAME_BEGIN;
const END: u32 = RSF_GAME_CPU_FRAME_END;
const SAMPLE: u32 = RSF_GAME_CPU_INPUT_SAMPLE;
const SIM_BEGIN: u32 = RSF_GAME_CPU_SIMULATION_BEGIN;
const SIM_END: u32 = RSF_GAME_CPU_SIMULATION_END;
const INPUT: u32 = RSF_GAME_CPU_INPUT_EVENT;
const PING: u32 = 0xc123;

pub(super) fn fact(session_id: u64, source_frame_id: u64, stage: u32, timestamp_qpc: u64) -> Event {
    Event {
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: RSF_GAME_CPU_INPUT_KEYED,
        ..cpu_event(session_id, source_frame_id, stage, timestamp_qpc, 1000)
    }
}

#[derive(Default)]
struct Delivery {
    events: Vec<Event>,
    future: Option<Event>,
    enqueued: i32,
}

unsafe extern "C" fn sink(user: *mut c_void, event: *const Event) {
    let delivery = user.cast::<Delivery>();
    let event = unsafe { *event };
    let future = {
        let delivery = unsafe { &mut *delivery };
        delivery.events.push(event);
        if event.stage == BEGIN {
            delivery.future.take()
        } else {
            None
        }
    };
    if let Some(future) = future {
        let accepted = unsafe { rsf_native_cpu_dispatch(&future, Some(sink), user) };
        unsafe { (*delivery).enqueued = accepted };
    }
}

impl Delivery {
    fn send(&mut self, event: &Event) -> bool {
        unsafe { rsf_native_cpu_dispatch(event, Some(sink), (self as *mut Self).cast()) == 1 }
    }

    fn stage(&mut self, source: u64, stage: u32, stamp: u64) {
        assert!(self.send(&fact(9, source, stage, stamp)));
    }
}

fn counts() -> rsf_native_cpu_counters {
    // The output structure contains only integer fields.
    let mut stats: rsf_native_cpu_counters = unsafe { std::mem::zeroed() };
    stats.struct_size = size_of_val(&stats) as u32;
    assert_eq!(unsafe { rsf_native_cpu_counters_get(&mut stats) }, 1);
    stats
}

fn check() -> i32 {
    let mut delivered = Delivery::default();
    #[repr(C, align(8))]
    struct Short([u32; 2]);
    let mut short = Short([4, u32::MAX]);
    let user = (&raw mut delivered).cast();
    assert_eq!(
        unsafe { rsf_native_cpu_dispatch((&raw const short).cast(), Some(sink), user) },
        0
    );
    assert_eq!(
        unsafe { rsf_native_cpu_read(9, 1, (&raw mut short).cast()) },
        0
    );
    assert_eq!(short.0, [4, u32::MAX]);

    #[repr(C)]
    struct Extended {
        value: Event,
        tail: u64,
    }
    let extended = Extended {
        value: Event {
            struct_size: size_of::<Extended>() as u32,
            ..fact(9, 1, BEGIN, 10)
        },
        tail: 123,
    };
    assert_eq!(
        unsafe { rsf_native_cpu_dispatch(std::ptr::from_ref(&extended).cast(), Some(sink), user) },
        1
    );
    assert_eq!(delivered.events.len(), 1);
    assert_eq!(delivered.events[0].struct_size as usize, size_of::<Event>());
    delivered.stage(1, SAMPLE, 20);
    delivered.stage(1, SIM_BEGIN, 21);
    let count = delivered.events.len();
    delivered.stage(1, SAMPLE, 22);
    delivered.stage(1, SIM_BEGIN, 22);
    assert_eq!(delivered.events.len(), count);
    for input_kind in [1, 1, 2, 4, 7] {
        assert!(delivered.send(&Event {
            input_kind,
            ..fact(9, 1, INPUT, 23)
        }));
    }
    unsafe { rsf_native_cpu_ping_message(PING) };
    let ping = Event {
        message_id: PING,
        ..fact(9, 1, INPUT, 24)
    };
    assert!(delivered.send(&ping));
    let after_ping = delivered.events.len();
    assert!(delivered.send(&ping));
    assert_eq!(delivered.events.len(), after_ping);
    delivered.stage(1, SIM_END, 30);
    for marker in 0..6 {
        assert_eq!(unsafe { rsf_native_cpu_marker(1, marker) }, 1);
    }
    assert_eq!(unsafe { rsf_native_cpu_marker(1, 5) }, 0);
    delivered.stage(1, END, 40);
    assert!(!delivered.send(&ping));
    let mut frame: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
    frame.struct_size = size_of_val(&frame) as u32;
    assert_eq!(unsafe { rsf_native_cpu_read(9, 1, &mut frame) }, 1);
    assert_eq!(
        (
            frame.input_mask,
            frame.input_events,
            frame.pings,
            frame.marker_mask
        ),
        (7, [1; 3], 1, 63)
    );

    delivered.events.clear();
    delivered.stage(2, BEGIN, 50);
    delivered.stage(2, SIM_END, 60);
    assert_eq!(delivered.events.len(), 4);
    for (event, stage) in delivered.events[1..3].iter().zip([SAMPLE, SIM_BEGIN]) {
        assert_eq!(event.stage, stage);
        assert_ne!(event.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE, 0);
    }
    for marker in [0, 2] {
        assert_eq!(unsafe { rsf_native_cpu_marker(2, marker) }, -1);
    }
    let stats = counts();
    assert_eq!(
        (
            stats.frames,
            stats.pings,
            stats.substitutes,
            stats.order_violations
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(stats.input_events, [1; 3]);
    assert!(stats.duplicates >= 5);
    delivered.stage(2, END, 70);

    let message = Event {
        message_id: PING,
        flags: RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE,
        ..fact(9, 0, INPUT, 75)
    };
    assert!(delivered.send(&message));
    delivered.events.clear();
    delivered.stage(3, BEGIN, 80);
    assert_eq!(delivered.events.len(), 2);
    assert_eq!(
        (
            delivered.events[1].source_frame_id,
            delivered.events[1].message_id
        ),
        (3, PING)
    );
    delivered.stage(3, END, 90);
    assert!(delivered.send(&message));
    delivered.events.clear();
    delivered.stage(4, BEGIN, 1000);
    assert_eq!(delivered.events.len(), 1);
    let stats = counts();
    assert_eq!((stats.expired_messages, stats.pings), (1, 2));
    delivered.stage(4, END, 1010);

    delivered.stage(5, BEGIN, 1100);
    delivered.stage(5, SIM_BEGIN, 1110);
    delivered.stage(5, SAMPLE, 1120);
    delivered.stage(5, SIM_END, 1130);
    let stats = counts();
    assert_eq!((stats.order_violations, stats.substitutes), (0, 1));
    let invalid = fact(9, 6, BEGIN, 1200);
    assert!(!delivered.send(&Event {
        event_version: 99,
        ..invalid
    }));
    assert!(!delivered.send(&Event {
        flags: 0x80000000,
        ..invalid
    }));
    assert!(delivered.send(&Event {
        event_version: 0,
        flags: 0,
        ..fact(9, 6, BEGIN, 1300)
    }));
    for marker in [4, 0] {
        assert_eq!(unsafe { rsf_native_cpu_marker(6, marker) }, -1);
    }
    assert_eq!(counts().order_violations, 1);
    delivered.stage(6, SAMPLE, 1310);
    delivered.stage(6, SIM_BEGIN, 1320);
    delivered.stage(6, SIM_END, 1330);
    for marker in [0, 2, 4, 5, 1, 3] {
        assert_eq!(unsafe { rsf_native_cpu_marker(6, marker) }, 1);
    }

    let outside = Event {
        flags: RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_INPUT_UNFRAMED,
        input_kind: 3,
        ..fact(9, 0, INPUT, 1340)
    };
    assert!(delivered.send(&outside));
    delivered.events.clear();
    delivered.stage(7, BEGIN, 1400);
    assert_eq!(delivered.events.len(), 3);
    assert_eq!(delivered.events[0].stage, END);
    assert_ne!(delivered.events[0].flags & RSF_GAME_CPU_DERIVED_BOUNDARY, 0);
    assert_eq!(
        (
            delivered.events[2].source_frame_id,
            delivered.events[2].timestamp_qpc
        ),
        (7, 1340)
    );
    let stats = counts();
    assert_eq!(stats.unframed_inputs, 1);
    assert!(stats.derived_frame_ends >= 1);
    delivered.stage(7, END, 1410);
    assert!(delivered.send(&outside));
    delivered.events.clear();
    delivered.stage(8, BEGIN, 2000);
    assert_eq!(delivered.events.len(), 1);
    assert_eq!(counts().expired_messages, 2);
    assert_eq!(unsafe { rsf_native_cpu_release_session(10) }, 0);
    assert_eq!(unsafe { rsf_native_cpu_read(9, 8, &mut frame) }, 1);
    assert_eq!(unsafe { rsf_native_cpu_release_session(9) }, 1);
    assert_eq!(unsafe { rsf_native_cpu_read(9, 8, &mut frame) }, 0);

    assert!(delivered.send(&fact(10, 1, BEGIN, 2100)));
    assert!(delivered.send(&fact(10, 1, END, 2101)));
    delivered.future = Some(Event {
        flags: RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_INPUT_UNFRAMED,
        input_kind: RSF_GAME_INPUT_MOUSE,
        ..fact(10, 0, INPUT, 2205)
    });
    delivered.events.clear();
    assert!(delivered.send(&fact(10, 2, BEGIN, 2200)));
    assert_eq!(delivered.enqueued, 1);
    assert_eq!(delivered.events.len(), 1);
    assert!(delivered.send(&fact(10, 2, END, 2206)));
    delivered.events.clear();
    assert!(delivered.send(&fact(10, 3, BEGIN, 2210)));
    assert_eq!(delivered.events.len(), 2);
    assert_eq!(
        (
            delivered.events[1].source_frame_id,
            delivered.events[1].timestamp_qpc
        ),
        (3, 2205)
    );
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latency_events_fixture() -> i32 {
    unsafe { rsf_native_cpu_release() };
    let result = crate::run(check);
    unsafe { rsf_native_cpu_release() };
    result
}
