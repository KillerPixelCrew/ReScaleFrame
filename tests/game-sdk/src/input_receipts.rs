// SPDX-License-Identifier: GPL-3.0-only
use crate::sys::*;
use std::ffi::c_void;

#[repr(C)]
struct ExtendedReceipt {
    packet: rsf_game_cpu_receipt_event,
    tail: u64,
}

#[derive(Default)]
struct Delivery {
    count: u32,
    malformed: u32,
}

unsafe extern "C" fn sink(user: *mut c_void, event: *const rsf_game_cpu_event) {
    let delivery = unsafe { &mut *user.cast::<Delivery>() };
    let event = unsafe { &*event };
    delivery.count += 1;
    if event.source_frame_id != 0
        || event.event_version != RSF_GAME_CPU_EVENT_VERSION_RECEIPT
        || event.struct_size as usize != size_of::<rsf_game_cpu_receipt_event>()
    {
        delivery.malformed += 1;
    }
}

fn check() -> i32 {
    assert_eq!(size_of::<rsf_game_cpu_receipt_event>(), 80);
    let mut delivery = Delivery::default();
    let user = (&raw mut delivery).cast();
    let dispatch = |header: *const rsf_game_cpu_event| unsafe {
        rsf_native_cpu_dispatch(header, Some(sink), user)
    };
    let begin = super::cpu_events::fact(7, 1, RSF_GAME_CPU_FRAME_BEGIN, 1);
    let mut extended = ExtendedReceipt {
        packet: rsf_game_cpu_receipt_event {
            header: rsf_game_cpu_event {
                struct_size: size_of::<ExtendedReceipt>() as u32,
                stage: RSF_GAME_CPU_INPUT_EVENT,
                source_frame_id: 0,
                timestamp_qpc: 2,
                render_expected: 0,
                input_kind: RSF_GAME_INPUT_MOUSE,
                event_version: RSF_GAME_CPU_EVENT_VERSION_RECEIPT,
                flags: RSF_GAME_CPU_INPUT_UNFRAMED,
                ..begin
            },
            receipt_id: 2,
            window_key: 123,
        },
        tail: 0,
    };
    assert_eq!(unsafe { rsf_native_cpu_event(&begin) }, 1);
    assert_eq!(dispatch(std::ptr::from_ref(&extended).cast()), 1);
    let packet = &mut extended.packet;
    packet.header.struct_size = size_of::<rsf_game_cpu_receipt_event>() as u32;
    packet.receipt_id = 1;
    assert_eq!(dispatch(std::ptr::from_ref(packet).cast()), 1);
    assert_eq!(dispatch(std::ptr::from_ref(packet).cast()), 0);
    packet.receipt_id = 3;
    packet.header.input_kind = 8;
    assert_eq!(dispatch(std::ptr::from_ref(packet).cast()), 0);
    packet.header.input_kind = RSF_GAME_INPUT_MOUSE;
    assert_eq!(dispatch(std::ptr::from_ref(packet).cast()), 1);
    packet.header.struct_size = size_of::<rsf_game_cpu_event>() as u32;
    assert_eq!(dispatch(std::ptr::from_ref(packet).cast()), 0);

    // Both output structures contain only integer fields.
    let mut counts: rsf_native_input_receipts = unsafe { std::mem::zeroed() };
    counts.struct_size = size_of_val(&counts) as u32;
    counts.version = 1;
    assert_eq!(unsafe { rsf_native_input_receipts_get(&mut counts) }, 1);
    assert_eq!(
        (counts.accepted, counts.duplicates, counts.rejected),
        (3, 1, 1)
    );
    assert_eq!(
        (counts.input_kinds[1], delivery.count, delivery.malformed),
        (3, 3, 0)
    );
    let mut frame: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
    frame.struct_size = size_of_val(&frame) as u32;
    assert_eq!(unsafe { rsf_native_cpu_read(7, 1, &mut frame) }, 1);
    assert_eq!(
        (
            frame.input_mask,
            frame.pings,
            frame.stage_mask,
            frame.marker_mask
        ),
        (0, 0, 1, 0)
    );
    unsafe { rsf_native_cpu_release() };
    assert_eq!(unsafe { rsf_native_input_receipts_get(&mut counts) }, 1);
    assert_eq!(counts.accepted, 0);
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_input_receipts_fixture() -> i32 {
    unsafe { rsf_native_cpu_release() };
    let result = crate::run(check);
    unsafe { rsf_native_cpu_release() };
    result
}
