// SPDX-License-Identifier: MIT
#pragma once

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/game_renderer.h>
#include <cstddef>
#include <cstdint>

// velocity_stream.rs owns the per-draw stream policy: scope and draw admission, the pipeline
// route, record bounds, indirect packing and argument range, capture and zero-source sizing,
// the hash build's count and dispatch and the packed b13 draw parameters. velocity_pass.cpp keeps
// every D3D11 call and carries out each decision.

// Bound stages besides vertex and pixel.
enum : uint32_t { VELOCITY_STAGE_GEOMETRY = 1, VELOCITY_STAGE_HULL = 2, VELOCITY_STAGE_DOMAIN = 4 };
// Mismatched: incomplete tessellation, or tessellation with a native GS.
enum : uint32_t { VELOCITY_ROUTE_ADMITTED = 0, VELOCITY_ROUTE_SKIPPED = 1, VELOCITY_ROUTE_MISMATCHED = 2 };

// The indirect validation's cbuffer (Capacity, OutputCapacity, Packing, Points).
struct VelocityIndirectBounds {
    uint32_t vertices, records, packing, points;
};
// capacity: what a capture or zero-source buffer holding fewer than bounds.records grows to.
// built: the hash build's record count. zero_source: copy the zero source over the capture first.
struct VelocityStreamPlan {
    VelocityIndirectBounds bounds;
    uint32_t capacity, built, zero_source, padding;
};
// The b13 cbuffer of the generated geometry, tessellation and pixel shaders.
struct VelocityDrawParameters {
    float rect[4], jitter[4];
    float cutoff;
    uint32_t history, mask, padding;
    float matrix[16];
};
// The hash build's cbuffer (Count, Mask, Mode, WorkWidth) and its direct dispatch.
struct VelocityHashParameters {
    uint32_t count, mask, mode, work_width;
};
struct VelocityHashDispatch {
    VelocityHashParameters parameters;
    uint32_t groups[2];
};

extern "C" {
// Nonzero when a primary translucent mesh scope with a camera draws single-sample with depth.
int32_t rsf_ac7_velocity_stream_draw(const rsf_game_render_pass* scope, const rsf_frame_tap_geometry* draw);
uint32_t rsf_ac7_velocity_stream_route(uint32_t topology, uint32_t stages);
// Nonzero with the plan written when the draw's records fit.
int32_t rsf_ac7_velocity_stream_plan(const rsf_frame_tap_geometry* draw, uint32_t stages,
    uint32_t geometry_capacity, uint32_t maximum_vertices, uint32_t points, VelocityStreamPlan* plan);
// The end of the argument copy, or zero when the buffer does not hold the draw's arguments.
uint32_t rsf_ac7_velocity_indirect_end(uint32_t kind, uint32_t offset, uint32_t byte_width);
// rect is the viewport's left, top, width and height. previous_records is zero without a
// previous history buffer. Nonzero for exactly one viewport of positive extent.
int32_t rsf_ac7_velocity_draw_parameters(const rsf_game_render_pass* scope, uint32_t viewports,
    const float (*rect)[4], uint32_t usable, uint32_t previous_records, VelocityDrawParameters* parameters);
// Nonzero when there are records to build. A dynamic build reads its count from the arguments.
int32_t rsf_ac7_velocity_hash_dispatch(uint32_t records, uint32_t destination_records, uint32_t mode,
    uint32_t dynamic, VelocityHashDispatch* dispatch);
}

static_assert(sizeof(VelocityIndirectBounds) == 16);
static_assert(sizeof(VelocityStreamPlan) == 32 && offsetof(VelocityStreamPlan, capacity) == 16);
static_assert(sizeof(VelocityDrawParameters) == 112 && offsetof(VelocityDrawParameters, cutoff) == 32 &&
    offsetof(VelocityDrawParameters, matrix) == 48);
static_assert(sizeof(VelocityHashParameters) == 16 && sizeof(VelocityHashDispatch) == 24);
