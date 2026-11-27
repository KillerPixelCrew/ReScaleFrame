// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between the Rust resolution owner and typed native calls in resolution.c.
#include "render_records.h"
#include <rescaleframe/game_renderer.h>

RSF_FALLOUT4_RENDER_ABI_BEGIN
typedef void (*ResolutionReport)(const char*);

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeOutputOwner) == 40 && offsetof(NativeOutputOwner, triangles) == 16);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeResolutionReceipt) == 128 && offsetof(NativeResolutionReceipt, input_ratio) == 32 &&
    offsetof(NativeResolutionReceipt, frame) == 72 && offsetof(NativeResolutionReceipt, evaluation) == 87 &&
    offsetof(NativeResolutionReceipt, actual_count) == 96);

int32_t rsf_fallout4_resolution_config(rsf_game_render_config_fn, void* user, rsf_game_render_config*) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_resolution_report_receipt(ResolutionReport, const rsf_game_render_pass*, void* context,
    const NativeResolutionReceipt*) RSF_FALLOUT4_RENDER_NOEXCEPT;

RSF_FALLOUT4_RENDER_ABI_END
