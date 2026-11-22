// SPDX-License-Identifier: MIT
#pragma once
#include "render_records.h"
#include <rescaleframe/game_renderer.h>

// Admission and composition live in depth_domains.rs. The native callers borrow these records.
RSF_FALLOUT4_RENDER_ABI_BEGIN
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeDepthCamera) == 71 * sizeof(float));
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeDepthSnapshot) == 1496);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeDepthSnapshot, native_frame) == 40 && offsetof(NativeDepthSnapshot, width) == 144 &&
    offsetof(NativeDepthSnapshot, mode) == 168 && offsetof(NativeDepthSnapshot, intervals) == 172 &&
    offsetof(NativeDepthSnapshot, near_camera) == 268 && offsetof(NativeDepthSnapshot, render_rect) == 1120 &&
    offsetof(NativeDepthSnapshot, descriptor_read) == 1224 && offsetof(NativeDepthSnapshot, cube_key) == 1232 &&
    offsetof(NativeDepthSnapshot, colour_slot) == 1248 && offsetof(NativeDepthSnapshot, colour_texture_key) == 1264 &&
    offsetof(NativeDepthSnapshot, scissor) == 1280 && offsetof(NativeDepthSnapshot, scissor_enabled) == 1296 &&
    offsetof(NativeDepthSnapshot, raster_valid) == 1300 && offsetof(NativeDepthSnapshot, maps) == 1304);

typedef void (*NativeDepthReport)(const char*);
bool rsf_fallout4_depth_capture_raster(const rsf_fallout4_cpu_identity*,
    NativeDepthSnapshot*, NativeDepthReport) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_depth_raster_current(const NativeDepthSnapshot*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_depth_capture(const rsf_fallout4_cpu_identity*,
    NativeDepthSnapshot*, NativeDepthReport) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_depth_make(const NativeDepthSnapshot*,
    const rsf_fallout4_cpu_identity*, const rsf_game_render_pass*, rsf_game_depth_domains*,
    NativeDepthReport) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_depth_report_refusal(NativeDepthReport, const char* stage,
    const NativeDepthSnapshot*, const NativeDepthMathFailure*, uint32_t math) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_depth_report_lease(NativeDepthReport, const NativeDepthSnapshot*,
    const NativeDepthSnapshot*, uint32_t reached, uint32_t passed, uint32_t stage,
    uint32_t changed) RSF_FALLOUT4_RENDER_NOEXCEPT;

RSF_FALLOUT4_RENDER_ABI_END
