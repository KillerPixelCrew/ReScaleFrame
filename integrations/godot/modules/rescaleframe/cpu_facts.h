// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
#include <stddef.h>

// Private record builder in the SCons Rust object. The caller supplies writable storage.
extern "C" void rsf_godot_build_cpu_event(rsf_game_cpu_event *out, uint32_t stage,
    uint64_t session_id, uint64_t source_frame_id, uint64_t timestamp_qpc, uint64_t qpc_frequency,
    uint32_t render_expected, uint32_t input_kind, uint32_t unframed_input);

static_assert(sizeof(rsf_game_cpu_event) == 64 && alignof(rsf_game_cpu_event) == 8 &&
    offsetof(rsf_game_cpu_event, struct_size) == 0 && offsetof(rsf_game_cpu_event, stage) == 4 &&
    offsetof(rsf_game_cpu_event, session_id) == 8 && offsetof(rsf_game_cpu_event, source_frame_id) == 16 &&
    offsetof(rsf_game_cpu_event, timestamp_qpc) == 24 && offsetof(rsf_game_cpu_event, qpc_frequency) == 32 &&
    offsetof(rsf_game_cpu_event, render_expected) == 40 && offsetof(rsf_game_cpu_event, input_kind) == 44 &&
    offsetof(rsf_game_cpu_event, message_id) == 48 && offsetof(rsf_game_cpu_event, event_version) == 52 &&
    offsetof(rsf_game_cpu_event, flags) == 56, "cpu_facts.rs mirrors the CPU event layout");
static_assert(RSF_GAME_CPU_EVENT_VERSION == 1 &&
    RSF_GAME_CPU_INPUT_KEYED == 1 && RSF_GAME_CPU_INPUT_UNFRAMED == 16,
    "cpu_facts.rs mirrors these CPU event constants");
