// SPDX-License-Identifier: MIT
#pragma once

#include <cstddef>

// velocity_renderer.rs owns the per-device renderer lifetime: the published renderer and its
// device, replacement when a draw arrives on another device, the variant, history and profile
// resets after a publication, and teardown at release. velocity_pass.cpp lends the leaves that
// create and destroy its native renderer. Its replay lock spans each borrow from lookup through
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
// Destroys the published renderer, then forgets the observed sources and layouts, the variant
// table, the history and the profile.
void rsf_ac7_velocity_renderer_release(void);
}

static_assert(sizeof(VelocityRendererLeaves) == 16 && offsetof(VelocityRendererLeaves, destroy) == 8);
