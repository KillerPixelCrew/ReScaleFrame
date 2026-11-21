// SPDX-License-Identifier: MIT

use core::mem::{align_of, offset_of, size_of};

// Matches game_renderer.h. The SCons object builds without the Cargo SDK crate.
const EVENT_VERSION: u32 = 1;
const INPUT_KEYED: u32 = 1;
const INPUT_UNFRAMED: u32 = 16;

#[repr(C)]
pub struct CpuEvent {
    struct_size: u32,
    stage: u32,
    session_id: u64,
    source_frame_id: u64,
    timestamp_qpc: u64,
    qpc_frequency: u64,
    render_expected: u32,
    input_kind: u32,
    message_id: u32,
    event_version: u32,
    flags: u32,
}

const _: () = {
    assert!(size_of::<CpuEvent>() == 64);
    assert!(align_of::<CpuEvent>() == 8);
    assert!(offset_of!(CpuEvent, struct_size) == 0);
    assert!(offset_of!(CpuEvent, stage) == 4);
    assert!(offset_of!(CpuEvent, session_id) == 8);
    assert!(offset_of!(CpuEvent, source_frame_id) == 16);
    assert!(offset_of!(CpuEvent, timestamp_qpc) == 24);
    assert!(offset_of!(CpuEvent, qpc_frequency) == 32);
    assert!(offset_of!(CpuEvent, render_expected) == 40);
    assert!(offset_of!(CpuEvent, input_kind) == 44);
    assert!(offset_of!(CpuEvent, message_id) == 48);
    assert!(offset_of!(CpuEvent, event_version) == 52);
    assert!(offset_of!(CpuEvent, flags) == 56);
};

/// # Safety
/// `out` must point to writable, aligned storage for one `rsf_game_cpu_event`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_build_cpu_event(
    out: *mut CpuEvent,
    stage: u32,
    session_id: u64,
    source_frame_id: u64,
    timestamp_qpc: u64,
    qpc_frequency: u64,
    render_expected: u32,
    input_kind: u32,
    unframed_input: u32,
) {
    let flags = if unframed_input != 0 {
        INPUT_KEYED | INPUT_UNFRAMED
    } else {
        INPUT_KEYED
    };
    let event = CpuEvent {
        struct_size: size_of::<CpuEvent>() as u32,
        stage,
        session_id,
        source_frame_id,
        timestamp_qpc,
        qpc_frequency,
        render_expected,
        input_kind,
        message_id: 0,
        event_version: EVENT_VERSION,
        flags,
    };
    // The native caller owns the record and reports it after this function returns.
    unsafe { out.write(event) };
}
