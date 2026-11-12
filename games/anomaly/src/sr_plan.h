// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR Stage 2 render/output plan (docs/research/anomaly-sr-seam.md section 10). Pure arithmetic, no engine or D3D
// dependency, so tests/anomaly_sr_plan.cpp checks every rounding rule without the game.
//
// The world window renders at render size: round(output * scale) per axis, with one scale for both axes, clamped to
// [min_scale, max_scale] and to [1, output]. Targets the engine creates at a fraction of the screen keep that
// fraction of the render size, computed with the engine's own integer division (rt_half_depth is Device.dwWidth / 2).
#include <cmath>
#include <cstdint>

namespace rsf::anomaly::sr {
inline constexpr float min_scale = 0.5f, max_scale = 1.0f, default_scale = 0.67f;

// NaN, infinities and values <= 0 select the default; everything else is clamped to [min_scale, max_scale].
inline float clamp_scale(float requested) noexcept {
    if (!(requested > 0.0f) || !std::isfinite(requested)) return default_scale;
    return requested < min_scale ? min_scale : requested > max_scale ? max_scale : requested;
}

// One axis. Rounds half up in double precision (1920 * 0.67 = 1286.4 -> 1286, 1080 * 0.67 = 723.6 -> 724), then
// keeps the result inside [ceil(output * min_scale), output] and never below 1.
inline uint32_t render_extent(uint32_t output, float scale) noexcept {
    if (!output) return 0;
    const double s = clamp_scale(scale);
    auto value = static_cast<uint32_t>(std::floor(double(output) * s + 0.5));
    const auto lowest = static_cast<uint32_t>(std::ceil(double(output) * min_scale));
    if (value < lowest) value = lowest;
    if (value > output) value = output;
    return value ? value : 1;
}

struct ScalePlan {
    uint32_t output_width{}, output_height{};
    uint32_t render_width{}, render_height{};
    float scale{1.0f};   // the clamped scale the plan was built from
    bool identity() const noexcept { return render_width == output_width && render_height == output_height; }
    bool operator==(const ScalePlan&) const = default;
};
// False for a zero output size (nothing to plan).
inline bool make_plan(uint32_t output_width, uint32_t output_height, float scale, ScalePlan& out) noexcept {
    out = {};
    if (!output_width || !output_height) return false;
    out.scale = clamp_scale(scale);
    out.output_width = output_width;
    out.output_height = output_height;
    out.render_width = render_extent(output_width, out.scale);
    out.render_height = render_extent(output_height, out.scale);
    return true;
}

// Stage 5: the render size a backend planned for this output (its quality level), held to the same window rules: one
// size per axis inside [ceil(output * min_scale), output]. False for a zero size or a plan outside those bounds, which
// the caller then does not use (the world window falls back to SRScale and the pipeline refuses that frame's size).
inline bool make_plan_for_render(uint32_t output_width, uint32_t output_height, uint32_t render_width, uint32_t render_height,
    ScalePlan& out) noexcept {
    out = {};
    if (!output_width || !output_height || !render_width || !render_height) return false;
    const auto lowest = [](uint32_t output) { return static_cast<uint32_t>(std::ceil(double(output) * min_scale)); };
    if (render_width > output_width || render_height > output_height || render_width < lowest(output_width) ||
        render_height < lowest(output_height))
        return false;
    out.output_width = output_width;
    out.output_height = output_height;
    out.render_width = render_width;
    out.render_height = render_height;
    out.scale = float(double(render_width) / double(output_width));
    return true;
}

// Size rule of a twinned engine target, from its created extent relative to the output extent.
enum class Extent : uint8_t { Full, Half };
// The twin extent for one axis: Full -> render, Half -> render / 2 (engine integer division). False when the engine
// extent is neither the output extent nor (when `half_allowed`) output / 2, or when the result would be zero.
inline bool twin_extent(uint32_t engine, uint32_t output, uint32_t render, bool half_allowed, uint32_t& twin, Extent& rule) noexcept {
    twin = 0;
    if (!engine || !output || !render || render > output) return false;
    if (engine == output) { rule = Extent::Full; twin = render; return true; }
    if (half_allowed && engine == output / 2 && render / 2) { rule = Extent::Half; twin = render / 2; return true; }
    return false;
}
}
