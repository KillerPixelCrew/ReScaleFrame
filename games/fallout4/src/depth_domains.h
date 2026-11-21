// SPDX-License-Identifier: MIT
#pragma once
#include "cpu_events.h"
#include <rescaleframe/game_renderer.h>
#include <cstddef>

// Admission and composition live in the rsf-fallout4 crate (depth_domains.rs). depth_domains.cpp
// supplies its native leaves (depth_domains_platform.h). These wrappers keep the C++ signatures.
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
static_assert(sizeof(NativeDepthCamera) == 71 * sizeof(float));
static_assert(sizeof(NativeDepthSnapshot) == 1304);
static_assert(offsetof(NativeDepthSnapshot, native_frame) == 40 && offsetof(NativeDepthSnapshot, width) == 144 &&
    offsetof(NativeDepthSnapshot, mode) == 168 && offsetof(NativeDepthSnapshot, intervals) == 172 &&
    offsetof(NativeDepthSnapshot, near_camera) == 268 && offsetof(NativeDepthSnapshot, render_rect) == 1120 &&
    offsetof(NativeDepthSnapshot, descriptor_read) == 1224 && offsetof(NativeDepthSnapshot, cube_key) == 1232 &&
    offsetof(NativeDepthSnapshot, colour_slot) == 1248 && offsetof(NativeDepthSnapshot, colour_texture_key) == 1264 &&
    offsetof(NativeDepthSnapshot, scissor) == 1280 && offsetof(NativeDepthSnapshot, scissor_enabled) == 1296 &&
    offsetof(NativeDepthSnapshot, raster_valid) == 1300);

using NativeDepthReport = void (*)(const char*);
struct NativeDepthLeaves;
// The process-lived leaf table of depth_domains.cpp.
const NativeDepthLeaves* native_depth_leaves() noexcept;

extern "C" {
bool rsf_fallout4_depth_verify() noexcept;
bool rsf_fallout4_depth_capture_raster(const NativeDepthLeaves*, const rsf_fallout4_cpu_identity*,
    NativeDepthSnapshot*, NativeDepthReport) noexcept;
bool rsf_fallout4_depth_raster_current(const NativeDepthLeaves*, const NativeDepthSnapshot*) noexcept;
bool rsf_fallout4_depth_snapshot_current(const NativeDepthLeaves*, const NativeDepthSnapshot*,
    NativeDepthReport) noexcept;
bool rsf_fallout4_depth_capture(const NativeDepthLeaves*, const rsf_fallout4_cpu_identity*,
    NativeDepthSnapshot*, NativeDepthReport) noexcept;
bool rsf_fallout4_depth_make(const NativeDepthLeaves*, const NativeDepthSnapshot*,
    const rsf_fallout4_cpu_identity*, const rsf_game_render_pass*, rsf_game_depth_domains*,
    NativeDepthReport) noexcept;
}

// Raster ownership deliberately does not require sampled CPU attribution or
// temporal matrices/history. It never establishes temporal admission.
inline bool capture_native_raster(const CpuIdentity& cpu, NativeDepthSnapshot& output,
    void (*report)(const char*) = nullptr) noexcept {
    const auto value = cpu_identity_abi(cpu);
    return rsf_fallout4_depth_capture_raster(native_depth_leaves(), &value, &output, report);
}
inline bool native_raster_snapshot_current(const NativeDepthSnapshot& snapshot) noexcept {
    return rsf_fallout4_depth_raster_current(native_depth_leaves(), &snapshot);
}
inline bool verify_native_depth_domains() noexcept { return rsf_fallout4_depth_verify(); }
inline bool native_depth_snapshot_current(const NativeDepthSnapshot& snapshot,
    void (*report)(const char*) = nullptr) noexcept {
    return rsf_fallout4_depth_snapshot_current(native_depth_leaves(), &snapshot, report);
}
// Caller must have returned from original prepass21F38B0 at exact call21F9F73
// (return21F9F78) inside the ordinary, unchanged CPU render source lease.
// Optional bounded first receipt per named refusal stage, matching the input reader.
inline bool capture_native_depth_domains(const CpuIdentity& cpu, NativeDepthSnapshot& output,
    void (*report)(const char*) = nullptr) noexcept {
    const auto value = cpu_identity_abi(cpu);
    return rsf_fallout4_depth_capture(native_depth_leaves(), &value, &output, report);
}
// Describes scalar depth at the composed raster samples, not spatial reprojection.
// Failure leaves output all zero; caller must refuse mapping rather than use raw depth.
inline bool make_native_depth_domains(const NativeDepthSnapshot& snapshot, const CpuIdentity& cpu,
    const rsf_game_render_pass& pass, rsf_game_depth_domains& output,
    void (*report)(const char*) = nullptr) noexcept {
    const auto value = cpu_identity_abi(cpu);
    return rsf_fallout4_depth_make(native_depth_leaves(), &snapshot, &value, &pass, &output, report);
}
}
