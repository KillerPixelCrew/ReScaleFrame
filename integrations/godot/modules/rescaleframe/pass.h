// SPDX-License-Identifier: MIT
#pragma once
#include "primary.h"
#include <rescaleframe/game_renderer.h>
#include <stddef.h>
#include <stdint.h>

// Private SR and final-scene pass records in the SCons Rust object (pass.rs). Rust decides the
// identity, keys, flags, screen class, driver route and camera scalars and keeps the last
// recorded SR pass. The C++ fills the camera matrices, copies the record into the public pass
// and sets the native resource pointers per route.

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
    uint32_t gameplay;
    uint32_t driver; // RSF_GODOT_DRIVER_* from admission
    uint32_t reserved;
};

// Rendering server thread.
extern "C" void rsf_godot_sr_plan(const rsf_godot_sr_facts *facts, rsf_godot_pass_record *out);
extern "C" void rsf_godot_sr_record(const rsf_godot_sr_facts *facts, const rsf_godot_pass_record *record);
extern "C" uint32_t rsf_godot_final_scene_plan(const rsf_godot_render_admission *facts, rsf_godot_pass_record *out);

static_assert(RSF_GAME_RENDER_SR == 1 && RSF_GAME_RENDER_FINAL_SCENE == 10 && RSF_GAME_RENDER_RESET == 1 &&
    RSF_GAME_RENDER_PRIMARY == 2 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4 && RSF_GAME_CAMERA_ABI_VERSION == 1,
    "pass.rs mirrors these values");
static_assert(sizeof(rsf_camera_frame) == 384 && alignof(rsf_camera_frame) == 4 &&
    offsetof(rsf_camera_frame, view_to_clip) == 8 && offsetof(rsf_camera_frame, clip_to_view) == 72 &&
    offsetof(rsf_camera_frame, view_to_world) == 136 && offsetof(rsf_camera_frame, world_to_view) == 200 &&
    offsetof(rsf_camera_frame, clip_to_previous_clip) == 264 && offsetof(rsf_camera_frame, jitter_pixels) == 328 &&
    offsetof(rsf_camera_frame, previous_jitter_pixels) == 336 && offsetof(rsf_camera_frame, render_width) == 344 &&
    offsetof(rsf_camera_frame, render_height) == 348 && offsetof(rsf_camera_frame, output_width) == 352 &&
    offsetof(rsf_camera_frame, output_height) == 356 && offsetof(rsf_camera_frame, near_plane) == 360 &&
    offsetof(rsf_camera_frame, far_plane) == 364 && offsetof(rsf_camera_frame, vertical_fov_radians) == 368 &&
    offsetof(rsf_camera_frame, depth_inverted) == 372 && offsetof(rsf_camera_frame, motion_jittered) == 376 &&
    offsetof(rsf_camera_frame, frame_time_seconds) == 380,
    "pass.rs mirrors the camera frame layout");
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
    offsetof(rsf_godot_sr_facts, gameplay) == 92 && offsetof(rsf_godot_sr_facts, driver) == 96,
    "pass.rs mirrors the SR facts layout");
