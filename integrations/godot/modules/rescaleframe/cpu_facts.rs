// SPDX-License-Identifier: MIT

use core::mem::size_of;

use crate::frame_event_abi::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_UNFRAMED,
    rsf_game_cpu_event,
};

/// # Safety
/// `out` must point to writable, aligned storage for one `rsf_game_cpu_event`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_build_cpu_event(
    out: *mut rsf_game_cpu_event,
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
        RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_INPUT_UNFRAMED
    } else {
        RSF_GAME_CPU_INPUT_KEYED
    };
    let event = rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage,
        session_id,
        source_frame_id,
        timestamp_qpc,
        qpc_frequency,
        render_expected,
        input_kind,
        message_id: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags,
    };
    // The native caller owns the record and reports it after this function returns.
    unsafe { out.write(event) };
}
