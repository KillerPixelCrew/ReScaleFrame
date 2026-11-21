// SPDX-License-Identifier: MIT
#pragma once

#include <cstddef>
#include <cstdint>

// velocity_history.rs owns model and view identity, frame rotation, history validity, record
// accounting, the written and commit state and epoch retirement. velocity_pass.cpp keeps the
// D3D11 buffers and textures at the slots it assigns and carries out each decision.

// particle drops the vertex factory and draw offsets from the identity. reset is the scope's
// RSF_GAME_RENDER_RESET. records is this callback's stream-output record bound.
struct VelocityModelDraw {
    uint64_t session_id, history_key, view_key, primitive_key, material_key, vertex_factory_key;
    uint32_t vertex_hash, geometry_hash, start;
    int32_t base_vertex;
    uint32_t particle, frame, reset, records;
};
// rotate: swap previous and current and clear the new current. capacity: the current buffer must
// hold at least this many records. Both apply whenever slot is not VELOCITY_NO_SLOT.
struct VelocityModelDecision {
    uint32_t slot, rotate, usable, capacity;
};
// width and height are the native depth extent at a draw. velocity_commit.rs builds a commit's
// identity itself.
struct VelocityViewDraw {
    uint64_t session_id, history_key, view_key, family_key;
    uint32_t frame, width, height, padding;
};
enum : uint32_t {
    VELOCITY_VIEW_UNAVAILABLE = 0,
    VELOCITY_VIEW_SAME_FRAME = 1, // valid when the stored native depth is the draw's
    VELOCITY_VIEW_REUSE = 2,
    VELOCITY_VIEW_ALLOCATE = 3,   // discard the slot's targets and create them at width x height
    VELOCITY_VIEW_COMMIT = 4,
};
struct VelocityViewDecision {
    uint32_t slot, action, width, height;
};
enum : uint32_t { VELOCITY_RETIRE_MODEL = 0, VELOCITY_RETIRE_VIEW = 1 };
constexpr uint32_t VELOCITY_NO_SLOT = UINT32_MAX;
// Releases one slot's native resources. Called after the history lock is released; must not throw.
using VelocityRetireSlot = void (*)(void* user, uint32_t kind, uint32_t slot);

extern "C" {
// Nonzero when the records fit. The decision is filled either way.
int32_t rsf_ac7_velocity_model_begin(const VelocityModelDraw* draw, VelocityModelDecision* decision);
void rsf_ac7_velocity_view_begin(const VelocityViewDraw* draw, VelocityViewDecision* decision);
// Every target of the slot exists and holds the draw's depth.
void rsf_ac7_velocity_view_ready(uint32_t slot, const VelocityViewDraw* draw);
void rsf_ac7_velocity_view_written(uint32_t slot);
// A commit finds its view through rsf_ac7_velocity_commit_view in velocity_commit.h.
void rsf_ac7_velocity_view_committed(uint32_t slot);
size_t rsf_ac7_velocity_model_count(void);
void rsf_ac7_velocity_history_frame_end(void* user, VelocityRetireSlot retire);
// The native slots were discarded with their renderer.
void rsf_ac7_velocity_history_reset(void);
}

static_assert(sizeof(VelocityModelDraw) == 80 && offsetof(VelocityModelDraw, vertex_hash) == 48 &&
    offsetof(VelocityModelDraw, records) == 76);
static_assert(sizeof(VelocityModelDecision) == 16);
static_assert(sizeof(VelocityViewDraw) == 48 && offsetof(VelocityViewDraw, frame) == 32);
static_assert(sizeof(VelocityViewDecision) == 16);
