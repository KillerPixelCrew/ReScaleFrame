// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between the Rust resolution owner and typed native calls in resolution.c.
#include "render_records.h"
#include <rescaleframe/game_renderer.h>

RSF_FALLOUT4_RENDER_ABI_BEGIN
typedef bool (*ResolutionReserve)(const rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
typedef void (*ResolutionReport)(const char*);

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeOutputOwner) == 40 && offsetof(NativeOutputOwner, triangles) == 16);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeResolutionReceipt) == 128 && offsetof(NativeResolutionReceipt, input_ratio) == 32 &&
    offsetof(NativeResolutionReceipt, frame) == 72 && offsetof(NativeResolutionReceipt, evaluation) == 87 &&
    offsetof(NativeResolutionReceipt, actual_count) == 96);

bool rsf_fallout4_resolution_reconfigure(rsf_game_process_upscale_fn, void* user, void* context,
    const rsf_game_render_pass* planning) RSF_FALLOUT4_RENDER_NOEXCEPT;
int32_t rsf_fallout4_resolution_config(rsf_game_render_config_fn, void* user, rsf_game_render_config*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_resolution_configure(const rsf_game_host_services*,
    const rsf_game_render_pass*, void*, ResolutionReserve) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_resolution_preflight(const rsf_game_render_pass*,
    void*, NativeOutputOwner*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_resolution_begin_output(const NativeDepthSnapshot*, const rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_resolution_native_dr(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_resolution_end_output(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_resolution_finish_source(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_resolution_report(uint32_t stage,
    const rsf_game_render_pass*, void*, bool published, int32_t result, ResolutionReport) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_resolution_report_receipt(ResolutionReport, const rsf_game_render_pass*, void* context,
    const NativeResolutionReceipt*) RSF_FALLOUT4_RENDER_NOEXCEPT;

RSF_FALLOUT4_RENDER_ABI_END
