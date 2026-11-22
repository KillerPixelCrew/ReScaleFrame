// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>

// Private record builder in the SCons Rust object. The caller supplies writable storage.
extern "C" void rsf_godot_build_cpu_event(rsf_game_cpu_event *out, uint32_t stage,
    uint64_t session_id, uint64_t source_frame_id, uint64_t timestamp_qpc, uint64_t qpc_frequency,
    uint32_t render_expected, uint32_t input_kind, uint32_t unframed_input);
