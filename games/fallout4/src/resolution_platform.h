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
};

struct NativeResolutionLeaves {
    uintptr_t (*tls_slots)();
    uint32_t (*screen_refusals)(const rsf_fallout4_cpu_identity*);
    bool (*reconfigure)(const rsf_game_host_services*, void*, const rsf_game_render_pass*);
    bool (*render_config)(const rsf_game_host_services*, rsf_game_render_config*);
    bool (*reserve)(ResolutionReserve, const rsf_game_render_pass*);
    void (*build_viewport)(uintptr_t routine, uintptr_t renderer, uintptr_t shadow,
        uint32_t width, uint32_t height);
    void (*report)(ResolutionReport, const rsf_game_render_pass*, void*, const NativeResolutionReceipt*);
};

static_assert(sizeof(NativeOutputOwner) == 40 && offsetof(NativeOutputOwner, triangles) == 16);
static_assert(sizeof(NativeResolutionReceipt) == 96 && offsetof(NativeResolutionReceipt, input_ratio) == 32 &&
    offsetof(NativeResolutionReceipt, frame) == 72 && offsetof(NativeResolutionReceipt, evaluation) == 87);
static_assert(sizeof(NativeResolutionLeaves) == 7 * sizeof(void*));

extern "C" {
bool rsf_fallout4_resolution_verify() noexcept;
bool rsf_fallout4_resolution_inputs(const rsf_game_render_pass*) noexcept;
bool rsf_fallout4_resolution_configure(const NativeResolutionLeaves*, const rsf_game_host_services*,
    const rsf_game_render_pass*, void*, ResolutionReserve) noexcept;
bool rsf_fallout4_resolution_preflight(const NativeResolutionLeaves*, const rsf_game_render_pass*,
    void*, NativeOutputOwner*) noexcept;
bool rsf_fallout4_resolution_begin_output(const NativeResolutionLeaves*, const NativeDepthLeaves*,
    const NativeDepthSnapshot*, const rsf_game_render_pass*) noexcept;
bool rsf_fallout4_resolution_native_dr(const NativeResolutionLeaves*) noexcept;
bool rsf_fallout4_resolution_native_viewport(bool enabled) noexcept;
bool rsf_fallout4_resolution_end_output(const NativeResolutionLeaves*) noexcept;
bool rsf_fallout4_resolution_finish_source(const NativeResolutionLeaves*) noexcept;
bool rsf_fallout4_resolution_busy() noexcept;
bool rsf_fallout4_resolution_output_busy() noexcept;
bool rsf_fallout4_resolution_refused() noexcept;
void rsf_fallout4_resolution_report(const NativeResolutionLeaves*, uint32_t stage,
    const rsf_game_render_pass*, void*, bool published, int32_t result, ResolutionReport) noexcept;
}
}
