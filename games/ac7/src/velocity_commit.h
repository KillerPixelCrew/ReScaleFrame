// SPDX-License-Identifier: MIT
#pragma once

#include "velocity_history.h"
#include <rescaleframe/game_renderer.h>
#include <cstddef>
#include <cstdint>

// velocity_commit.rs owns the commit pass's admission and view identity, the motion target's
// extent, sample, array and format check, the refusal and commit log budgets and the per-frame CPU
// profile with its report thresholds. velocity_pass.cpp keeps the native depth identity, the
// target's description and device check, the merge and copies, and the clock.

// The native motion target's D3D11_TEXTURE2D_DESC Width, Height, SampleDesc.Count, ArraySize and
// Format.
struct VelocityCommitTarget {
    uint32_t width, height, samples, array_size, format;
};
// One admitted draw callback: total, variant admission and resource preparation time, its native
// frame and its record bound.
struct VelocityProfileSample {
    double elapsed_ms, shader_ms, resource_ms;
    uint32_t frame, records;
};
extern "C" {
// Nonzero with the slot and extent when the pass leases depth and motion and its view has motion
// written for its frame, view and family. The decision is unavailable otherwise.
int32_t rsf_ac7_velocity_commit_view(const rsf_game_render_pass* pass, VelocityViewDecision* decision);
// Nonzero for a single-sample, unarrayed R16G16 UNORM or typeless target at the view's extent.
int32_t rsf_ac7_velocity_commit_target(const VelocityViewDecision* view, const VelocityCommitTarget* target);
void rsf_ac7_velocity_log_refusal(const char* reason);
void rsf_ac7_velocity_log_commit(void);
void rsf_ac7_velocity_profile_sample(const VelocityProfileSample* sample);
void rsf_ac7_velocity_log_frame_end(uint32_t shader_objects);
// velocity_renderer.rs resets the profile when it publishes or releases a renderer.
}

static_assert(sizeof(VelocityCommitTarget) == 20);
static_assert(sizeof(VelocityProfileSample) == 32 && offsetof(VelocityProfileSample, frame) == 24);
