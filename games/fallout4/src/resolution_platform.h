// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between the Rust resolution owner and typed native calls in resolution.cpp.
#include "render_records.h"
#include <rescaleframe/game_renderer.h>
#include <cstddef>

namespace rsf::fallout4 {
struct NativeDepthSnapshot;

using ResolutionReserve = bool (*)(const rsf_game_render_pass*);
using ResolutionReport = void (*)(const char*);

static_assert(sizeof(NativeOutputOwner) == 40 && offsetof(NativeOutputOwner, triangles) == 16);
static_assert(sizeof(NativeResolutionReceipt) == 128 && offsetof(NativeResolutionReceipt, input_ratio) == 32 &&
    offsetof(NativeResolutionReceipt, frame) == 72 && offsetof(NativeResolutionReceipt, evaluation) == 87 &&
    offsetof(NativeResolutionReceipt, actual_count) == 96);

extern "C" {
bool rsf_fallout4_resolution_reconfigure(const rsf_game_host_services*, void* context,
    const rsf_game_render_pass* planning) noexcept;
bool rsf_fallout4_resolution_configure(const rsf_game_host_services*,
    const rsf_game_render_pass*, void*, ResolutionReserve) noexcept;
bool rsf_fallout4_resolution_preflight(const rsf_game_render_pass*,
    void*, NativeOutputOwner*) noexcept;
bool rsf_fallout4_resolution_begin_output(const NativeDepthSnapshot*, const rsf_game_render_pass*) noexcept;
bool rsf_fallout4_resolution_native_dr() noexcept;
bool rsf_fallout4_resolution_end_output() noexcept;
bool rsf_fallout4_resolution_finish_source() noexcept;
void rsf_fallout4_resolution_report(uint32_t stage,
    const rsf_game_render_pass*, void*, bool published, int32_t result, ResolutionReport) noexcept;
}
}
