// SPDX-License-Identifier: MIT
#pragma once

#include <cstddef>
#include <cstdint>
#include <rescaleframe/ac7_velocity_pass.h>

// velocity_renderer.rs owns the per-device renderer lifetime: the published renderer and its
// device, replacement when a draw arrives on another device, the variant, history and profile
// resets after a publication, and teardown at release. velocity_pass.cpp lends the leaves that
// create and destroy its native renderer. The Rust replay lock spans each borrow from lookup through
// the last D3D11 use and serializes publication, replacement, frame retirement and release.

// create returns an initialized renderer for the device, or null. destroy deletes one that create
// made. Neither may throw.
struct VelocityRendererLeaves {
    void* (*create)(void* device);
    void (*destroy)(void* renderer);
};

extern "C" {
// The renderer for the device. A different device gets a new one, which replaces and destroys the
// published renderer and resets the variant table, history and profile. Null when the device is
// null or creation failed; a failed creation keeps the published renderer.
void* rsf_ac7_velocity_renderer_admit(void* device, const VelocityRendererLeaves* leaves);
// The published renderer, or null.
void* rsf_ac7_velocity_renderer_published(void);
// The published renderer when it was made for this device, or null.
void* rsf_ac7_velocity_renderer_for(void* device);
// Synchronous submission leaves. All except merge borrow the replay lock.
void rsf_ac7_velocity_replay_draw(const rsf_frame_tap_geometry* draw,
    void (*submit)(const rsf_game_render_pass*, const rsf_frame_tap_geometry*));
int rsf_ac7_velocity_replay_commit(void* context, const rsf_game_render_pass* pass,
    int (*submit)(void*, const rsf_game_render_pass*));
void rsf_ac7_velocity_replay_merge(void* context, void (*draw)(void*));
void rsf_ac7_velocity_replay_frame_end(void (*retire)(void));
uint32_t rsf_ac7_velocity_replay_count(uint32_t (*count)(void));
}

static_assert(sizeof(VelocityRendererLeaves) == 16 && offsetof(VelocityRendererLeaves, destroy) == 8);
