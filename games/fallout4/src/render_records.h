// SPDX-License-Identifier: MIT
#pragma once
// Private C records shared with Rust. These are not DLL exports.
#include "cpu_events_platform.h"
#include <stddef.h>
#include <stdint.h>

// The records and callbacks are plain C. The macros stay so the declarations keep their shape.
#define RSF_FALLOUT4_RENDER_ABI_BEGIN
#define RSF_FALLOUT4_RENDER_ABI_END
#define RSF_FALLOUT4_RENDER_NOEXCEPT
#define RSF_FALLOUT4_RENDER_ASSERT(condition) _Static_assert(condition, #condition)

RSF_FALLOUT4_RENDER_ABI_BEGIN

typedef struct NativeScreen {
    uint32_t screen, facts, refusals;
    uint32_t camera_index;
    uintptr_t camera_state;
} NativeScreen;

typedef struct NativeOutputOwner {
    uint64_t context, manager, triangles[3];
} NativeOutputOwner;

typedef struct NativeResolutionReceipt {
    const char* stage;
    uintptr_t geometry[3];
    float input_ratio[2], ratio[2], raster[6];
    uint32_t frame, dirty;
    int32_t result;
    uint8_t viewport, active, partial;
    bool evaluation, published, input, output, changed, scalars, owner, shadow;
    uint32_t actual_count, actual_read;
    float actual_raster[6];
} NativeResolutionReceipt;

typedef struct NativeDepthMathFailure {
    uint32_t index;
    float actual, expected, relative_error;
    double w[4], minimum_w, range;
} NativeDepthMathFailure;

// Texture slots of the reader lease. Slot 0 also holds the framebuffer texture.
#define native_input_colour 0u
#define native_input_motion 1u
#define native_input_depth 2u
#define native_input_refraction_normals 3u
#define native_input_pipboy 4u
#define native_input_vats 5u
#define native_input_slot_count 6u
#define native_input_no_slot 0xffffffffu

typedef struct NativeDepthCamera {
    float view[16], projection[16], view_projection[16];
    float view_projection_unjittered[16], origin[3], port[4];
} NativeDepthCamera;

// Retained only after validation of a temporal snapshot.
typedef struct NativeDepthMaps {
    float near_map[4][4];
    float world_map[4][4];
    float projection_map[4][4];
} NativeDepthMaps;

// Stack-owned values only. Integer keys are compared as opaque identities;
// they confer no resource lifetime and are never dereferenced from this copy.
typedef struct NativeDepthSnapshot {
    rsf_fallout4_cpu_identity identity;
    uint64_t native_frame, renderer_key, context_key, device_context_key;
    uint64_t device_key, window_key, chain_key, graph_key;
    uint64_t near_camera_key, world_camera_key, cache_key, depth_key, depth_srv_key;
    uint32_t width, height, cache_count, depth_slot, taa, disable;
    unsigned char mode;
    float intervals[4], jitter[2], ratio[2], near_inverse_transposed[16];
    NativeDepthCamera near_camera, world_camera, world_unjittered_camera;
    // Producer-only raster, copied after prepass. Rect is integer LTRB sample coverage.
    int32_t render_rect[4];
    float actual_raster[6], shadow_raster[6], normalized_port[4];
    uint32_t shadow_dirty, colour_selector, depth_selector, cube_selector;
    uint32_t descriptor_width, descriptor_height;
    bool descriptor_read;
    uint64_t cube_key, depth_dsv_key;
    // Logical viewport owner and submitted slot0 physical owner are distinct.
    uint32_t colour_slot, bound_colour_slot, bound_depth_slot;
    uint64_t colour_texture_key, colour_rtv_key;
    int32_t scissor[4];
    bool scissor_enabled;
    unsigned char default_viewport, dynamic_viewport;
    bool valid, raster_valid;
    NativeDepthMaps maps;
} NativeDepthSnapshot;

typedef struct NativeInputState {
    uintptr_t renderer, camera, cache, context, device, chain, window;
    uint32_t frame, width, height, count, taa, disable, colour, motion, depth;
    float ratio[2], viewport[4], delta_seconds;
    unsigned char default_viewport, dynamic_viewport;
} NativeInputState;

typedef struct NativeCamera {
    float view[16], projection[16];
    float view_projection_unjittered[16], previous_view_projection_unjittered[16];
    float position_adjust[3], previous_position_adjust[3], jitter_ndc[2];
} NativeCamera;

typedef struct CameraRaster {
    float xywh[4];
    // Integer pixel edges, LTRB; extents are right-left and bottom-top.
    int32_t crop[4];
} CameraRaster;

typedef struct NativeInputSwapFacts {
    uint64_t window;
    uint32_t read, width, height, reserved;
} NativeInputSwapFacts;

// The resource behind one shader-resource or render-target view. identity is the viewed
// resource's; pointer is the ID3D11Texture2D query result, 0 when the resource is missing or the
// query failed. device and the allocation fields describe that texture. view_mip is MostDetailedMip
// for a shader-resource view and MipSlice for a render-target view; view_mips is 0 for the latter.
typedef struct NativeInputResourceFacts {
    uint64_t identity, pointer, device;
    uint32_t read, width, height, format, mip_levels, array_size;
    uint32_t sample_count, sample_quality, bind_flags;
    uint32_t view_format, view_dimension, view_mip, view_mips, reserved;
} NativeInputResourceFacts;

// Every local the reader receipt prints, as Rust last wrote it.
typedef struct NativeInputReceipt {
    NativeInputState before;
    NativeCamera native;
    NativeInputResourceFacts views[3], rtv;
    NativeInputSwapFacts swap;
    uint64_t expected_renderer, selected, last_key, expected[3];
    uint32_t after_frame, scanned, camera_matches, matching_jitter, last_jitter;
    uint32_t viewport_count, confirmed_viewport_count, camera_read;
    uint32_t resource_match[3], device_match[3];
    float viewport[6], confirmed_viewport[6];
} NativeInputReceipt;

typedef struct NativeInputHistoryReceipt {
    uint32_t frame, previous_frame, reset, reasons;
    uint32_t width, height, previous_width, previous_height;
    CameraRaster raster, previous_raster;
} NativeInputHistoryReceipt;

// The framebuffer receipt. Allocation, view and swap fields are filled at the reader stage that
// queried them.
typedef struct NativeFramebufferReceipt {
    NativeInputState before, after;
    NativeInputSwapFacts swap;
    uint64_t target_texture, target_rtv, leased;
    uint32_t platform, confirmed_platform;
    uint32_t width, height, mip_levels, array_size, sample_count, format;
    uint32_t rtv_dimension, rtv_mip;
} NativeFramebufferReceipt;

RSF_FALLOUT4_RENDER_ABI_END
