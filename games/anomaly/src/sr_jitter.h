// SPDX-License-Identifier: MIT
#pragma once
// Native adapters for the Rust jitter owner.
//
// Units and signs. A jitter sample is a subpixel offset in RENDER pixels, +X right, +Y down: the rasterized image
// moves by (+x, +y) pixels. This is the RSF camera convention (rsf_camera_frame::jitter_pixels, game_frame.h; the AC7
// producer derives the same value as clip_x * width / 2, clip_y * -height / 2), so the future SR pass receives these
// numbers unchanged. In NDC the offset is (2x / W, -2y / H); in the motion PS b13 it is (x / W, y / H) in viewport UV,
// Y down (shaders/core/README.md).
//
// Engine matrices are X-Ray Fmatrix bytes: row-major storage and ROW vectors, clip = v * M. The NDC offset is applied
// by the independent temporal fixture (column vectors) on the transposed matrix, which for row vectors is
// M[i][0] += 2x/W * M[i][3] and M[i][1] -= 2y/H * M[i][3] for every row i (for a perspective matrix: _31 and _32).
#include "sr_stage_platform.h"
#include <bit>
#include <array>
#include <cstdint>
#include <optional>

namespace rsf::anomaly::sr {
// ---- Halton (2, 3) sequence -----------------------------------------------------------------------------------------
struct JitterSample {
    float x{}, y{};                 // render pixels, in [-0.5, 0.5)
    uint32_t index{}, phase_count{8};
};
// Sample `index` of a sequence of `phase_count`: Halton(index mod phases + 1, 2/3) - 0.5. Index 0 is (0, -1/6).
inline JitterSample jitter_sample(uint32_t index, uint32_t phase_count) noexcept {
    return std::bit_cast<JitterSample>(rsf_anomaly_sr_jitter_sample_at(index, phase_count));
}

// The main-view sequence. Restarts at index 0 when the render or output size, the device reset generation or the
// scale changes, and when the caller reports a history break (frames without a world in between: level load,
// loading screens, the main menu). Scope (SVP) frames are not passed in, so they never advance it.
struct JitterKey {
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
    uint64_t generation{};
};
class JitterSequence {
public:
    JitterSequence() noexcept { state_.phases = 8; }
    JitterSample next(const JitterKey& key, bool continuous) noexcept {
        const uint32_t dimensions[4]{key.render_width, key.render_height, key.output_width, key.output_height};
        return std::bit_cast<JitterSample>(rsf_anomaly_sr_sequence_next(&state_, &dimensions, key.generation, continuous));
    }
    bool last_was_reset() const noexcept { return state_.reset; }
private:
    rsf_anomaly_sr_jitter_sequence state_{};
};

// ---- engine (row-vector) matrices -----------------------------------------------------------------------------------
using EngineMatrix = std::array<float, 16>;   // Fmatrix: _11.._14, _21.._24, _31.._34, _41.._44
extern "C" bool rsf_anomaly_jitter_projection(const float*, float, float, uint32_t, uint32_t, float*) noexcept;
inline std::optional<EngineMatrix> production_jitter_projection(const EngineMatrix& projection, float x, float y,
    uint32_t render_width, uint32_t render_height) noexcept {
    EngineMatrix result{};
    if (!rsf_anomaly_jitter_projection(projection.data(), x, y, render_width, render_height, result.data()))
        return std::nullopt;
    return result;
}

// ---- mip bias ------------------------------------------------------------------------------------------------------
// Added to the sampler cache's current bias (the user's r__tf_mipbias) for the world window: log2(render / output)
// plus `offset` (SRMipBias), clamped to [-3, 1]. Rendering at a fraction of the output samples mips chosen for that
// fraction; log2 of the ratio restores the output's mip selection, the offset is the reconstruction policy.
// Default 0: with -0.3 the ground texture aliased into per-frame grain under the jitter, and DLSS presets J and K
// kept the history of far walking characters on that grain as a trail behind them (FSR and XeSS did not).
inline constexpr float min_mip_bias_offset = RSF_ANOMALY_SR_MIN_MIP_OFFSET;
inline constexpr float max_mip_bias_offset = RSF_ANOMALY_SR_MAX_MIP_OFFSET;
inline constexpr float default_mip_bias_offset = RSF_ANOMALY_SR_DEFAULT_MIP_OFFSET;
inline float window_mip_bias_delta(uint32_t render_width, uint32_t output_width, float offset) noexcept {
    return rsf_anomaly_sr_mip_delta(render_width, output_width, offset);
}

// ---- what the adapter publishes for the motion stage and the SR pass ---------------------------------------------
// Updated at every H1 (phase_scene_prepare prologue), before any world draw of that frame. A scope (SVP) frame
// publishes zeros: its history is separate and never jittered.
struct JitterState {
    uint64_t frame{};                         // main-view frames that reached H1 (the SR history counter)
    uint32_t sequence_index{}, phase_count{8};
    float current_x{}, current_y{};           // applied this frame, render pixels (0 when not applied)
    float previous_x{}, previous_y{};         // applied in the previous main-view frame
    uint32_t render_width{}, render_height{};
    uint32_t previous_render_width{}, previous_render_height{};
    bool scope{}, applied{}, previous_applied{}, reset{};
};
struct JitterUv {
    float current_u{}, current_v{}, previous_u{}, previous_v{};   // viewport UV, Y down (core PS b13 rsf_jitter)
};
static_assert(sizeof(JitterState) == sizeof(rsf_anomaly_sr_jitter_state) &&
    offsetof(JitterState, reset) == offsetof(rsf_anomaly_sr_jitter_state, reset));
inline JitterUv jitter_uv(const JitterState& s) noexcept {
    const auto state = std::bit_cast<rsf_anomaly_sr_jitter_state>(s);
    rsf_anomaly_sr_jitter_uv uv{};
    rsf_anomaly_sr_jitter_to_uv(&state, &uv);
    return {uv.current_u, uv.current_v, uv.previous_u, uv.previous_v};
}
inline JitterState next_state(const JitterState& last_main, bool scope, bool applied, const JitterSample& sample,
    uint32_t render_width, uint32_t render_height, bool reset) noexcept {
    const auto last = std::bit_cast<rsf_anomaly_sr_jitter_state>(last_main);
    const auto next_sample = std::bit_cast<rsf_anomaly_sr_jitter_sample>(sample);
    rsf_anomaly_sr_jitter_state out{};
    rsf_anomaly_sr_next_jitter(&last, scope, applied, &next_sample, render_width, render_height, reset, &out);
    return std::bit_cast<JitterState>(out);
}
}
