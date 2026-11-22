// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr.h>

// An ordered receipt means any native writes are ordered on the immediate
// context. The host still checks device life before retiring resources.
rsf_dlss_pipeline_result rsf_native_sr_evaluate_ordered(void* context,
    const rsf_game_render_pass* pass, uint32_t* ordered);
// Rust host ingress. Keep exceptions from native SR leaves on the C++ side.
extern "C" int32_t rsf_native_sr_evaluate_ordered_guarded(void* context,
    const rsf_game_render_pass* pass, uint32_t* ordered) noexcept;
