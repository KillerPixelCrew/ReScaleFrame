// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
#include <stddef.h>
#include <stdint.h>

// Private CPU frame lifecycle and render frame identity in the SCons Rust object (frame.rs).
// Rust keeps the state and decides the order; these native callbacks do the engine work.
struct rsf_godot_frame_callbacks {
    uint64_t (*now_usec)();
    void (*report)(uint32_t stage, uint64_t source_frame_id, uint32_t render_expected,
        uint32_t input_kind, uint32_t unframed_input);
    void (*queue_render)(uint64_t source_frame_id, double cpu_seconds);
};

struct rsf_godot_render_frame {
    uint64_t source_frame_id; // zero when the queued draw does not present
    double frame_seconds;
    uint32_t gameplay;
    uint32_t sequence_break; // the last SR evaluation was not the preceding frame
};

// Main loop thread.
extern "C" void rsf_godot_cpu_begin(const rsf_godot_frame_callbacks *callbacks);
extern "C" void rsf_godot_cpu_simulation_begin(const rsf_godot_frame_callbacks *callbacks);
extern "C" void rsf_godot_cpu_queue(const rsf_godot_frame_callbacks *callbacks, uint32_t present);
extern "C" void rsf_godot_cpu_end(const rsf_godot_frame_callbacks *callbacks);
extern "C" void rsf_godot_cpu_input(const rsf_godot_frame_callbacks *callbacks, uint32_t kind);

// Render thread.
extern "C" void rsf_godot_render_frame_begin(uint64_t source_frame_id, uint32_t gameplay, double seconds);
extern "C" void rsf_godot_render_frame_read(rsf_godot_render_frame *out);
extern "C" void rsf_godot_render_frame_evaluated();
extern "C" void rsf_godot_render_frame_retarget();

static_assert(sizeof(rsf_godot_frame_callbacks) == 3 * sizeof(void *), "frame.rs mirrors the callbacks");
static_assert(sizeof(rsf_godot_render_frame) == 24 && alignof(rsf_godot_render_frame) == 8 &&
    offsetof(rsf_godot_render_frame, source_frame_id) == 0 && offsetof(rsf_godot_render_frame, frame_seconds) == 8 &&
    offsetof(rsf_godot_render_frame, gameplay) == 16 && offsetof(rsf_godot_render_frame, sequence_break) == 20,
    "frame.rs mirrors the render frame layout");
static_assert(RSF_GAME_CPU_FRAME_BEGIN == 0 && RSF_GAME_CPU_INPUT_SAMPLE == 1 &&
    RSF_GAME_CPU_SIMULATION_BEGIN == 2 && RSF_GAME_CPU_SIMULATION_END == 3 &&
    RSF_GAME_CPU_FRAME_END == 4 && RSF_GAME_CPU_INPUT_EVENT == 5,
    "frame.rs mirrors these CPU stages");
