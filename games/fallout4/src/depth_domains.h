// SPDX-License-Identifier: GPL-3.0-only
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
    bool valid{};
};
bool verify_native_depth_domains() noexcept;
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
