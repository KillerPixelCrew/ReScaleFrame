// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>

// Private primary viewport identity, render admission and SR history reset policy in the SCons
// Rust object (primary.rs). RIDs cross as RID::get_id(); the C++ side keeps the typed RIDs.

enum : uint32_t {
    RSF_GODOT_DRIVER_OTHER = 0,
    RSF_GODOT_DRIVER_D3D12 = 1,
    RSF_GODOT_DRIVER_VULKAN = 2,
};

struct rsf_godot_render_admission {
    uint64_t source_frame_id;
    uint64_t render_target; // zero without render buffers
    uint32_t host_active;   // native host ready and enabled
    uint32_t buffers_valid;
    uint32_t view_count;    // zero without render buffers
    uint32_t driver;        // RSF_GODOT_DRIVER_*
};

struct rsf_godot_sr_history {
    uint64_t color; // internal color texture
    int32_t internal_width;
    int32_t internal_height;
    double frame_seconds;
    double camera_distance; // previous recorded camera origin to the current one
    uint32_t sequence_break;
    uint32_t reserved;
};

// Rendering server thread unless noted. pass.h decides, records and admits the SR and
// final-scene passes through primary.rs's crate functions.
extern "C" void rsf_godot_primary_set(uint64_t viewport, uint64_t target);
extern "C" uint64_t rsf_godot_primary_viewport();
extern "C" uint32_t rsf_godot_primary_sizing(uint32_t host_active, uint64_t target);
extern "C" void rsf_godot_reset_request(); // any thread
extern "C" void rsf_godot_camera_changed(uint32_t host_active, uint64_t target);
extern "C" uint32_t rsf_godot_sr_admit(const rsf_godot_render_admission *facts);

static_assert(sizeof(rsf_godot_render_admission) == 32 && alignof(rsf_godot_render_admission) == 8 &&
    offsetof(rsf_godot_render_admission, source_frame_id) == 0 && offsetof(rsf_godot_render_admission, render_target) == 8 &&
    offsetof(rsf_godot_render_admission, host_active) == 16 && offsetof(rsf_godot_render_admission, buffers_valid) == 20 &&
    offsetof(rsf_godot_render_admission, view_count) == 24 && offsetof(rsf_godot_render_admission, driver) == 28,
    "primary.rs mirrors the admission layout");
static_assert(sizeof(rsf_godot_sr_history) == 40 && alignof(rsf_godot_sr_history) == 8 &&
    offsetof(rsf_godot_sr_history, color) == 0 && offsetof(rsf_godot_sr_history, internal_width) == 8 &&
    offsetof(rsf_godot_sr_history, internal_height) == 12 && offsetof(rsf_godot_sr_history, frame_seconds) == 16 &&
    offsetof(rsf_godot_sr_history, camera_distance) == 24 && offsetof(rsf_godot_sr_history, sequence_break) == 32,
    "primary.rs mirrors the history layout");
