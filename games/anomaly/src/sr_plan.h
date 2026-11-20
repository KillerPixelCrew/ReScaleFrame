// SPDX-License-Identifier: MIT
#pragma once
// SR Stage 2 render/output plan (docs/research/anomaly-sr-seam.md section 10). Pure arithmetic, no engine or D3D
// dependency, so tests/anomaly_sr_plan.cpp checks every rounding rule without the game.
//
// The world window renders at render size: round(output * scale) per axis, with one scale for both axes, clamped to
// [min_scale, max_scale] and to [1, output]. Targets the engine creates at a fraction of the screen keep that
// fraction of the render size, computed with the engine's own integer division (rt_half_depth is Device.dwWidth / 2);
// sr_swap.rs applies that twin rule.
#include "sr_stage_platform.h"
#include <cstdint>

namespace rsf::anomaly::sr {
// 0.33 admits Ultra Performance (a third of the output, 853 of 2560). Stage 2 was measured from 0.5 up.
inline constexpr float min_scale = RSF_ANOMALY_SR_MIN_SCALE;
inline constexpr float max_scale = RSF_ANOMALY_SR_MAX_SCALE;
inline constexpr float default_scale = RSF_ANOMALY_SR_DEFAULT_SCALE;

inline float clamp_scale(float requested) noexcept { return rsf_anomaly_sr_clamp_scale(requested); }
inline uint32_t render_extent(uint32_t output, float scale) noexcept { return rsf_anomaly_sr_render_extent(output, scale); }

struct ScalePlan {
    uint32_t output_width{}, output_height{};
    uint32_t render_width{}, render_height{};
    float scale{1.0f};   // the clamped scale the plan was built from
    bool identity() const noexcept { return render_width == output_width && render_height == output_height; }
    bool operator==(const ScalePlan&) const = default;
};
inline bool make_plan(uint32_t output_width, uint32_t output_height, float scale, ScalePlan& out) noexcept {
    rsf_anomaly_sr_plan plan{};
    const bool accepted = rsf_anomaly_sr_make_plan(output_width, output_height, scale, &plan);
    out = {plan.output_width, plan.output_height, plan.render_width, plan.render_height, plan.scale};
    return accepted;
}

// Stage 5 uses the backend's requested extent when the world window can render it.
inline bool make_plan_for_render(uint32_t output_width, uint32_t output_height, uint32_t render_width, uint32_t render_height,
    ScalePlan& out) noexcept {
    rsf_anomaly_sr_plan plan{};
    const bool accepted = rsf_anomaly_sr_plan_for_render(output_width, output_height, render_width, render_height, &plan);
    out = {plan.output_width, plan.output_height, plan.render_width, plan.render_height, plan.scale};
    return accepted;
}
}
