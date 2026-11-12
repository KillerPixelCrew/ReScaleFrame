// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR Stage 3 jitter and mip bias arithmetic (docs/research/anomaly-sr-seam.md sections 4 and 11). Pure, no engine or
// D3D dependency, so tests/anomaly_sr_jitter.cpp checks every rule without the game.
//
// Units and signs. A jitter sample is a subpixel offset in RENDER pixels, +X right, +Y down: the rasterized image
// moves by (+x, +y) pixels. This is the RSF camera convention (rsf_camera_frame::jitter_pixels, game_frame.h; the AC7
// producer derives the same value as clip_x * width / 2, clip_y * -height / 2), so the future SR pass receives these
// numbers unchanged. In NDC the offset is (2x / W, -2y / H); in the motion PS b13 it is (x / W, y / H) in viewport UV,
// Y down (shaders/core/README.md).
//
// Engine matrices are X-Ray Fmatrix bytes: row-major storage and ROW vectors, clip = v * M. The NDC offset is applied
// by temporal::jitter_projection (column vectors) on the transposed matrix, which for row vectors is
// M[i][0] += 2x/W * M[i][3] and M[i][1] -= 2y/H * M[i][3] for every row i (for a perspective matrix: _31 and _32).
#include "../temporal/math.h"
#include <array>
#include <cmath>
#include <cstdint>
#include <optional>

namespace rsf::anomaly::sr {
// ---- Halton (2, 3) sequence -----------------------------------------------------------------------------------------
// Radical inverse of `index` (>= 1) in `base`.
inline double halton(uint32_t index, uint32_t base) noexcept {
    double result = 0.0, fraction = 1.0;
    while (index > 0) {
        fraction /= base;
        result += fraction * (index % base);
        index /= base;
    }
    return result;
}
// Phase count: ceil(8 * (output / render)^2) per axis ratio, the common practice (FSR 2 ffxFsr2GetJitterPhaseCount),
// clamped to [8, 64]. 1920 -> 1286 and 1280 -> 858 (scale 0.67) give 18, scale 0.5 gives 32, scale 1.0 gives 8.
inline uint32_t jitter_phase_count(uint32_t render_width, uint32_t output_width) noexcept {
    if (!render_width || !output_width || render_width > output_width) return 8;
    const double ratio = double(output_width) / double(render_width);
    const auto count = static_cast<uint32_t>(std::ceil(8.0 * ratio * ratio - 1e-9));
    return count < 8 ? 8 : count > 64 ? 64 : count;
}
struct JitterSample {
    float x{}, y{};                 // render pixels, in [-0.5, 0.5)
    uint32_t index{}, phase_count{8};
    bool operator==(const JitterSample&) const = default;
};
// Sample `index` of a sequence of `phase_count`: Halton(index mod phases + 1, 2/3) - 0.5. Index 0 is (0, -1/6).
inline JitterSample jitter_sample(uint32_t index, uint32_t phase_count) noexcept {
    if (!phase_count) phase_count = 8;
    const uint32_t i = index % phase_count;
    return {static_cast<float>(halton(i + 1, 2) - 0.5), static_cast<float>(halton(i + 1, 3) - 0.5), i, phase_count};
}

// The main-view sequence. Restarts at index 0 when the render or output size, the device reset generation or the
// scale changes, and when the caller reports a history break (frames without a world in between: level load,
// loading screens, the main menu). Scope (SVP) frames are not passed in, so they never advance it.
struct JitterKey {
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
    uint64_t generation{};
    bool operator==(const JitterKey&) const = default;
};
class JitterSequence {
public:
    // The sample for the next main-view world frame.
    JitterSample next(const JitterKey& key, bool continuous) noexcept {
        const uint32_t phases = jitter_phase_count(key.render_width, key.output_width);
        if (!started_ || !(key == key_) || !continuous || phases != phases_) {
            if (started_) ++resets_;
            started_ = true;
            key_ = key;
            phases_ = phases;
            index_ = 0;
            reset_ = true;
        } else reset_ = false;
        const auto sample = jitter_sample(index_, phases_);
        index_ = (index_ + 1) % phases_;
        ++frames_;
        return sample;
    }
    bool last_was_reset() const noexcept { return reset_; }
    uint64_t resets() const noexcept { return resets_; }
    uint64_t frames() const noexcept { return frames_; }
    uint32_t phase_count() const noexcept { return phases_; }
    // Index of the sample next() returns next.
    uint32_t next_index() const noexcept { return index_; }
    void clear() noexcept { *this = {}; }
private:
    JitterKey key_{};
    bool started_{}, reset_{};
    uint32_t phases_{8}, index_{};
    uint64_t resets_{}, frames_{};
};

// ---- burst capture gating -----------------------------------------------------------------------------------------------
// A Stage 3 jitter burst must hold consecutive, clean main-view frames. The wave 6 game run started it while the "Press any
// key to continue" screen was up: the world still renders under that screen (frames reach H1, so frames_without_world does
// not move), and every final image was the loading screen. The adapter reads the engine's loading-screen flag and feeds it
// here with its count of frames that never reached H1 (menu, loading, first frame after a reset).
class BurstGate {
public:
    static constexpr uint32_t default_settle_frames = 30;
    struct Verdict {
        bool capture{};   // this frame may be captured
        bool discard{};   // frames already taken are no longer consecutive or clean: drop them
    };
    BurstGate(uint32_t settle_frames = default_settle_frames) noexcept : settle_(settle_frames) {}
    // One call per main-view frame at H1. A loading screen or any frame without a world since the previous call breaks the
    // run; a burst starts only after `settle_frames` unbroken frames and, once started, continues while they stay unbroken.
    Verdict frame(bool loading_screen, uint64_t frames_without_world, bool frames_taken) noexcept {
        const bool broken = loading_screen || !marked_ || frames_without_world != mark_;
        mark_ = frames_without_world;
        marked_ = true;
        if (broken) {
            clear_ = 0;
            ++held_;
            if (frames_taken) ++restarts_;
            return {false, frames_taken};
        }
        ++clear_;
        return {frames_taken || clear_ >= settle_, false};
    }
    // The loading screen showed up while the frame already armed was in flight (checked before Present).
    void interrupted() noexcept { clear_ = 0; ++held_; ++restarts_; }
    uint64_t clear_frames() const noexcept { return clear_; }
    uint64_t held() const noexcept { return held_; }          // frames a capture was held back or broken
    uint64_t restarts() const noexcept { return restarts_; }  // bursts that lost frames and started over
private:
    uint32_t settle_{};
    uint64_t mark_{}, clear_{}, held_{}, restarts_{};
    bool marked_{};
};

// ---- engine (row-vector) matrices -----------------------------------------------------------------------------------
using EngineMatrix = std::array<float, 16>;   // Fmatrix: _11.._14, _21.._24, _31.._34, _41.._44
inline temporal::Mat4 to_column(const EngineMatrix& engine) noexcept {
    temporal::Mat4 m;
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c) m.values[r * 4 + c] = engine[c * 4 + r];
    return m;
}
inline EngineMatrix from_column(const temporal::Mat4& m) noexcept {
    EngineMatrix engine{};
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c) engine[c * 4 + r] = static_cast<float>(m.values[r * 4 + c]);
    return engine;
}
// The jittered copy of an unjittered engine projection, or nothing for a non-finite matrix or an invalid extent.
// Elements the jitter does not change are returned bit-exact (float -> double -> float).
inline std::optional<EngineMatrix> jitter_engine_projection(const EngineMatrix& projection, float x, float y,
    uint32_t render_width, uint32_t render_height) noexcept try {
    const auto jittered = temporal::jitter_projection(to_column(projection), {x, y}, {render_width, render_height});
    if (!jittered) return std::nullopt;
    auto result = from_column(*jittered);
    for (const float value : result) if (!std::isfinite(value)) return std::nullopt;
    return result;
} catch (...) { return std::nullopt; }
// Fmatrix::mul(A, B) (xrCore/_matrix.h:116), in the engine's summation order: m[i][j] = sum_k A[k][j] * B[i][k].
// The engine's Device.mFullTransform is mul(mProject, mView).
inline EngineMatrix engine_mul(const EngineMatrix& a, const EngineMatrix& b) noexcept {
    EngineMatrix m{};
    for (std::size_t i = 0; i < 4; ++i)
        for (std::size_t j = 0; j < 4; ++j)
            m[i * 4 + j] = a[0 * 4 + j] * b[i * 4 + 0] + a[1 * 4 + j] * b[i * 4 + 1] + a[2 * 4 + j] * b[i * 4 + 2] +
                a[3 * 4 + j] * b[i * 4 + 3];
    return m;
}

// ---- first-person sites (rmNear callback) ---------------------------------------------------------------------------
// The six first-person sites rebuild Device.mProject with build_projection (so _31 = _32 = 0), upload it and call
// rmNear; the shared rmNear slot then jitters that fresh matrix with the same pixel offset. A projection the adapter
// already wrote (the jittered main projection, or the last first-person matrix it jittered) is never jittered again,
// and a projection with a nonzero _31/_32 that the adapter did not write is left alone.
enum class FirstPersonAction : uint8_t { Jitter, AlreadyJittered, Foreign };
inline FirstPersonAction first_person_action(const EngineMatrix& current, const EngineMatrix& main_jittered,
    const EngineMatrix* last_written) noexcept {
    if (current == main_jittered || (last_written && current == *last_written)) return FirstPersonAction::AlreadyJittered;
    if (current[8] == 0.0f && current[9] == 0.0f) return FirstPersonAction::Jitter;
    return FirstPersonAction::Foreign;
}

// ---- mip bias ------------------------------------------------------------------------------------------------------
// Added to the sampler cache's current bias (the user's r__tf_mipbias) for the world window: log2(render / output)
// plus `offset` (SRMipBias), clamped to [-3, 1]. Rendering at a fraction of the output samples mips chosen for that
// fraction; log2 of the ratio restores the output's mip selection, the offset is the reconstruction policy.
// Default 0: with -0.3 the ground texture aliased into per-frame grain under the jitter, and DLSS presets J and K
// kept the history of far walking characters on that grain as a trail behind them (FSR and XeSS did not).
inline constexpr float min_mip_bias_offset = -2.0f, max_mip_bias_offset = 1.0f, default_mip_bias_offset = 0.0f;
inline float clamp_mip_bias_offset(float value) noexcept {
    if (!std::isfinite(value)) return default_mip_bias_offset;
    return value < min_mip_bias_offset ? min_mip_bias_offset : value > max_mip_bias_offset ? max_mip_bias_offset : value;
}
inline float window_mip_bias_delta(uint32_t render_width, uint32_t output_width, float offset) noexcept {
    if (!render_width || !output_width) return 0.0f;
    const double delta = std::log2(double(render_width) / double(output_width)) + clamp_mip_bias_offset(offset);
    return static_cast<float>(delta < -3.0 ? -3.0 : delta > 1.0 ? 1.0 : delta);
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
inline JitterUv jitter_uv(const JitterState& s) noexcept {
    JitterUv uv{};
    if (s.applied && s.render_width && s.render_height) {
        uv.current_u = s.current_x / float(s.render_width);
        uv.current_v = s.current_y / float(s.render_height);
    }
    if (s.previous_applied && s.previous_render_width && s.previous_render_height) {
        uv.previous_u = s.previous_x / float(s.previous_render_width);
        uv.previous_v = s.previous_y / float(s.previous_render_height);
    }
    return uv;
}
// Advance the published state for one H1. `applied` = the window opened and the projection carries `sample`.
inline JitterState next_state(const JitterState& last_main, bool scope, bool applied, const JitterSample& sample,
    uint32_t render_width, uint32_t render_height, bool reset) noexcept {
    JitterState s{};
    if (scope) { s = last_main; s.scope = true; s.applied = s.previous_applied = false; s.current_x = s.current_y = 0;
        s.previous_x = s.previous_y = 0; return s; }
    s.frame = last_main.frame + 1;
    s.sequence_index = sample.index;
    s.phase_count = sample.phase_count;
    s.applied = applied;
    s.current_x = applied ? sample.x : 0.0f;
    s.current_y = applied ? sample.y : 0.0f;
    s.render_width = render_width;
    s.render_height = render_height;
    s.previous_applied = last_main.applied;
    s.previous_x = last_main.current_x;
    s.previous_y = last_main.current_y;
    s.previous_render_width = last_main.render_width;
    s.previous_render_height = last_main.render_height;
    s.reset = reset;
    return s;
}
}
