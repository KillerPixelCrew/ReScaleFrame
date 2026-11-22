// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between the Rust resolution owner and typed native calls in resolution.cpp.
#include "cpu_events_platform.h"
#include "resolution.h"
#include <cstddef>

namespace rsf::fallout4 {
using ResolutionReserve = bool (*)(const rsf_game_render_pass&);
using ResolutionReport = void (*)(const char*);
struct NativeDepthLeaves;

struct NativeResolutionReceipt {
    const char* stage;
    uintptr_t geometry[3];
    float input_ratio[2], ratio[2], raster[6];
    uint32_t frame, dirty;
    int32_t result;
    uint8_t viewport, active, partial;
    bool evaluation, published, input, output, changed, scalars, owner, shadow;
    uint32_t actual_count, actual_read;
    float actual_raster[6];
};

struct NativeResolutionLeaves {
    uint32_t (*screen_refusals)(const rsf_fallout4_cpu_identity*);
    bool (*reconfigure)(const rsf_game_host_services*, void*, const rsf_game_render_pass*);
    bool (*render_config)(const rsf_game_host_services*, rsf_game_render_config*);
    bool (*reserve)(ResolutionReserve, const rsf_game_render_pass*);
    void (*report)(ResolutionReport, const rsf_game_render_pass*, void*, const NativeResolutionReceipt*);
};

static_assert(sizeof(NativeOutputOwner) == 40 && offsetof(NativeOutputOwner, triangles) == 16);
static_assert(sizeof(NativeResolutionReceipt) == 128 && offsetof(NativeResolutionReceipt, input_ratio) == 32 &&
    offsetof(NativeResolutionReceipt, frame) == 72 && offsetof(NativeResolutionReceipt, evaluation) == 87 &&
    offsetof(NativeResolutionReceipt, actual_count) == 96);
static_assert(sizeof(NativeResolutionLeaves) == 5 * sizeof(void*));

extern "C" {
bool rsf_fallout4_resolution_inputs(const rsf_game_render_pass*) noexcept;
bool rsf_fallout4_resolution_configure(const NativeResolutionLeaves*, const rsf_game_host_services*,
    const rsf_game_render_pass*, void*, ResolutionReserve) noexcept;
bool rsf_fallout4_resolution_preflight(const rsf_game_render_pass*,
    void*, NativeOutputOwner*) noexcept;
bool rsf_fallout4_resolution_begin_output(const NativeDepthLeaves*,
    const NativeDepthSnapshot*, const rsf_game_render_pass*) noexcept;
bool rsf_fallout4_resolution_native_dr() noexcept;
bool rsf_fallout4_resolution_end_output() noexcept;
bool rsf_fallout4_resolution_finish_source() noexcept;
void rsf_fallout4_resolution_report(const NativeResolutionLeaves*, uint32_t stage,
    const rsf_game_render_pass*, void*, bool published, int32_t result, ResolutionReport) noexcept;
}
}
