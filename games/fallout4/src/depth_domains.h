// SPDX-License-Identifier: MIT
#pragma once
#include "cpu_events.h"
#include <rescaleframe/game_renderer.h>

namespace rsf::fallout4 {
struct NativeDepthCamera {
    float view[16]{}, projection[16]{}, view_projection[16]{};
    float view_projection_unjittered[16]{}, origin[3]{}, port[4]{};
};
// Stack-owned values only. Integer keys are compared as opaque identities;
// they confer no resource lifetime and are never dereferenced from this copy.
struct NativeDepthSnapshot {
    CpuIdentity identity{};
    uint64_t native_frame{}, renderer_key{}, context_key{}, device_context_key{};
    uint64_t device_key{}, window_key{}, chain_key{}, graph_key{};
    uint64_t near_camera_key{}, world_camera_key{}, cache_key{}, depth_key{}, depth_srv_key{};
    uint32_t width{}, height{}, cache_count{}, depth_slot{}, taa{}, disable{};
    unsigned char mode{};
    float intervals[4]{}, jitter[2]{}, ratio[2]{}, near_inverse_transposed[16]{};
    NativeDepthCamera near_camera{}, world_camera{}, world_unjittered_camera{};
    // Producer-only raster, copied after prepass. Rect is integer LTRB sample coverage.
    int32_t render_rect[4]{};
    float actual_raster[6]{}, shadow_raster[6]{}, normalized_port[4]{};
    uint32_t shadow_dirty{}, colour_selector{}, depth_selector{}, cube_selector{};
    uint32_t descriptor_width{}, descriptor_height{};
    bool descriptor_read{};
    uint64_t cube_key{}, depth_dsv_key{};
    // Logical viewport owner and submitted slot0 physical owner are distinct.
    uint32_t colour_slot{}, bound_colour_slot{}, bound_depth_slot{};
    uint64_t colour_texture_key{}, colour_rtv_key{};
    int32_t scissor[4]{};
    bool scissor_enabled{};
    unsigned char default_viewport{}, dynamic_viewport{};
    bool valid{}, raster_valid{};
};
// Raster ownership deliberately does not require sampled CPU attribution or
// temporal matrices/history. It never establishes temporal admission.
bool capture_native_raster(const CpuIdentity&, NativeDepthSnapshot&,
    void (*report)(const char*) = nullptr) noexcept;
bool native_raster_snapshot_current(const NativeDepthSnapshot&) noexcept;
bool verify_native_depth_domains() noexcept;
bool native_depth_snapshot_current(const NativeDepthSnapshot&) noexcept;
// Caller must have returned from original prepass21F38B0 at exact call21F9F73
// (return21F9F78) inside the ordinary, unchanged CPU render source lease.
// Optional bounded first receipt per named refusal stage, matching the input reader.
bool capture_native_depth_domains(const CpuIdentity&, NativeDepthSnapshot&,
    void (*report)(const char*) = nullptr) noexcept;
// Describes scalar depth at the composed raster samples, not spatial reprojection.
// Failure leaves output all zero; caller must refuse mapping rather than use raw depth.
bool make_native_depth_domains(const NativeDepthSnapshot&, const CpuIdentity&,
    const rsf_game_render_pass&, rsf_game_depth_domains&,
    void (*report)(const char*) = nullptr) noexcept;
}
