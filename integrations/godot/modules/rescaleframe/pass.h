// SPDX-License-Identifier: MIT
#pragma once
#include "primary.h"
#include <rescaleframe/game_renderer.h>
#include <stddef.h>
#include <stdint.h>

// Private SR and final-scene pass records in the SCons Rust object (pass.rs). Rust decides the
// identity, keys, flags, screen class, driver route and camera scalars and keeps the last
// recorded SR pass. The C++ fills the camera matrices and supplies driver resource facts.

struct rsf_godot_pass_record {
    uint64_t session_id;
    uint64_t source_frame_id;
    uint64_t key; // view, history and viewport key: the primary render target
    uint32_t role;
    uint32_t flags;
    uint32_t resource_generation;
    uint32_t screen;
    uint32_t driver;           // RSF_GODOT_DRIVER_* of this route
    uint32_t normalized_depth; // the SR depth input is the motion pass's R32 copy
    float motion_to_uv[2];
    uint32_t motion_camera_included;
    uint32_t camera_valid;
    float previous_clip_to_clip[16];
    rsf_camera_frame camera;
    uint64_t reset_generation;
};

struct rsf_godot_sr_facts {
    rsf_godot_sr_history history;
    uint64_t source_frame_id;
    uint64_t render_target;
    int32_t output_width;
    int32_t output_height;
    float near_plane;
    float far_plane;
    float vertical_fov_radians;
    float jitter[2];          // taa_jitter
    float previous_jitter[2]; // prev_taa_jitter
    rsf_screen_class screen;
    uint32_t driver; // RSF_GODOT_DRIVER_* from admission
    uint32_t reserved;
};

// Rendering server thread.
extern "C" void rsf_godot_sr_plan(const rsf_godot_sr_facts *facts, rsf_godot_pass_record *out);
extern "C" void rsf_godot_sr_record(const rsf_godot_sr_facts *facts, const rsf_godot_pass_record *record);
extern "C" uint32_t rsf_godot_final_scene_plan(const rsf_godot_render_admission *facts, rsf_godot_pass_record *out);

static_assert(sizeof(rsf_godot_pass_record) == 520 && alignof(rsf_godot_pass_record) == 8 &&
    offsetof(rsf_godot_pass_record, key) == 16 && offsetof(rsf_godot_pass_record, role) == 24 &&
    offsetof(rsf_godot_pass_record, screen) == 36 && offsetof(rsf_godot_pass_record, driver) == 40 &&
    offsetof(rsf_godot_pass_record, motion_to_uv) == 48 && offsetof(rsf_godot_pass_record, camera_valid) == 60 &&
    offsetof(rsf_godot_pass_record, previous_clip_to_clip) == 64 && offsetof(rsf_godot_pass_record, camera) == 128 &&
    offsetof(rsf_godot_pass_record, reset_generation) == 512,
    "pass.rs mirrors the pass record layout");
static_assert(sizeof(rsf_godot_sr_facts) == 104 && alignof(rsf_godot_sr_facts) == 8 &&
    offsetof(rsf_godot_sr_facts, source_frame_id) == 40 && offsetof(rsf_godot_sr_facts, render_target) == 48 &&
    offsetof(rsf_godot_sr_facts, output_width) == 56 && offsetof(rsf_godot_sr_facts, near_plane) == 64 &&
    offsetof(rsf_godot_sr_facts, jitter) == 76 && offsetof(rsf_godot_sr_facts, previous_jitter) == 84 &&
    offsetof(rsf_godot_sr_facts, screen) == 92 && offsetof(rsf_godot_sr_facts, driver) == 96,
    "pass.rs mirrors the SR facts layout");
