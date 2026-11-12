// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 3 jitter and mip bias arithmetic (games/anomaly/src/sr_jitter.h) without the game: the Halton(2,3) sequence
// and its phase count and resets, the NDC offset applied to X-Ray row-vector projections, Fmatrix::mul, the
// first-person no-double-jitter rule, the mip bias delta, the state published to the motion stage and the SR pass, and
// the CPU form of the motion PS jitter cancellation (a static scene under jitter has zero motion).
#include "sr_jitter.h"

#include <cmath>
#include <cstdio>
#include <limits>
#include <set>
#include <string_view>

namespace {
using namespace rsf::anomaly;
using namespace rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)
bool near(double a, double b, double tolerance) { return std::fabs(a - b) <= tolerance; }

EngineMatrix perspective(float fov, float aspect, float near_plane = 0.2f, float far_plane = 1000.0f) {
    const float h = 1.0f / std::tan(fov * 0.5f), w = h / aspect, q = far_plane / (far_plane - near_plane);
    return {w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * near_plane, 0};
}
// Row vector times an engine matrix, in double.
std::array<double, 4> row_times(const std::array<double, 4>& v, const EngineMatrix& m) {
    std::array<double, 4> out{};
    for (int c = 0; c < 4; ++c)
        for (int r = 0; r < 4; ++r) out[c] += v[r] * double(m[r * 4 + c]);
    return out;
}
// Render-pixel position of a clip point (top-left origin, Y down), as the rasterizer maps NDC.
std::array<double, 2> pixel(const std::array<double, 4>& clip, uint32_t w, uint32_t h) {
    return {(clip[0] / clip[3] * 0.5 + 0.5) * w, (0.5 - clip[1] / clip[3] * 0.5) * h};
}

void halton_sequence() {
    CHECK(halton(1, 2) == 0.5 && halton(2, 2) == 0.25 && halton(3, 2) == 0.75 && halton(4, 2) == 0.125);
    CHECK(near(halton(1, 3), 1.0 / 3, 1e-15) && near(halton(2, 3), 2.0 / 3, 1e-15) && near(halton(3, 3), 1.0 / 9, 1e-15));
    const auto first = jitter_sample(0, 8);
    CHECK(first.x == 0.0f && near(first.y, -1.0 / 6, 1e-7) && first.index == 0 && first.phase_count == 8);
    // Every sample lies in [-0.5, 0.5); the first 8 x samples hit every 1/8 stratum once, the first 9 y every 1/9.
    std::set<int> x_strata, y_strata;
    for (uint32_t i = 0; i < 9; ++i) {
        const auto s = jitter_sample(i, 64);
        CHECK(s.x >= -0.5f && s.x < 0.5f && s.y >= -0.5f && s.y < 0.5f);
        if (i < 8) x_strata.insert(int(std::floor(halton(i + 1, 2) * 8 + 1e-9)));
        y_strata.insert(int(std::floor(halton(i + 1, 3) * 9 + 1e-9)));
        CHECK(near(s.x, halton(i + 1, 2) - 0.5, 1e-7) && near(s.y, halton(i + 1, 3) - 0.5, 1e-7));
    }
    CHECK(x_strata.size() == 8 && y_strata.size() == 9);
    // The index wraps at the phase count.
    CHECK(jitter_sample(18, 18) == jitter_sample(0, 18) && jitter_sample(19, 18).index == 1);
    CHECK(jitter_sample(5, 0).phase_count == 8);
    // Phase count: ceil(8 * (output / render)^2), clamped to [8, 64].
    CHECK(jitter_phase_count(1286, 1920) == 18 && jitter_phase_count(858, 1280) == 18);
    CHECK(jitter_phase_count(960, 1920) == 32 && jitter_phase_count(1920, 1920) == 8 && jitter_phase_count(1500, 1920) == 14);
    CHECK(jitter_phase_count(0, 1920) == 8 && jitter_phase_count(2000, 1920) == 8 && jitter_phase_count(100, 1920) == 64);
}

void sequence_resets() {
    JitterSequence s;
    const JitterKey key{1286, 724, 1920, 1080, 0};
    auto a = s.next(key, true);
    CHECK(a.index == 0 && a.phase_count == 18 && s.last_was_reset() && s.resets() == 0);
    for (uint32_t i = 1; i < 18; ++i) {
        const auto b = s.next(key, true);
        CHECK(b.index == i && !s.last_was_reset());
    }
    CHECK(s.next(key, true).index == 0 && !s.last_was_reset());   // wrapped, not reset
    CHECK(s.next(key, true).index == 1);
    // A history break (frames without a world in between: level load, loading screen, menu) restarts the sequence.
    CHECK(s.next(key, false).index == 0 && s.last_was_reset() && s.resets() == 1);
    CHECK(s.next(key, true).index == 1);
    // Scale change (render size), resolution change and device reset each restart it.
    const auto c = s.next({960, 540, 1920, 1080, 0}, true);
    CHECK(c.index == 0 && c.phase_count == 32 && s.resets() == 2);
    CHECK(s.next({1280, 720, 1280, 720, 0}, true).phase_count == 8 && s.resets() == 3);
    CHECK(s.next({1280, 720, 1280, 720, 1}, true).index == 0 && s.resets() == 4);
    CHECK(s.next({1280, 720, 1280, 720, 1}, true).index == 1 && s.next_index() == 2 && s.frames() == 26);
    s.clear();
    CHECK(s.next(key, true).index == 0 && s.resets() == 0);
}

void ndc_offset() {
    const uint32_t w = 1286, h = 724;
    const auto p = perspective(1.2f, 1920.0f / 1080.0f);
    for (const auto& s : {jitter_sample(0, 18), jitter_sample(1, 18), jitter_sample(7, 18), JitterSample{-0.5f, 0.4999f}}) {
        const auto j = jitter_engine_projection(p, s.x, s.y, w, h);
        CHECK(j.has_value());
        // Row vectors: only _31 and _32 change for a perspective matrix (_34 = 1, _44 = 0); everything else bit-exact.
        for (std::size_t i = 0; i < 16; ++i)
            if (i != 8 && i != 9) CHECK((*j)[i] == p[i]);
        CHECK(near((*j)[8], 2.0 * s.x / w, 1e-8) && near((*j)[9], -2.0 * s.y / h, 1e-8));
        // Points at several depths move by exactly (+x, +y) render pixels.
        for (const auto& v : {std::array<double, 4>{0, 0, 5, 1}, {-3.5, 2.25, 40, 1}, {12, -7, 900, 1}, {0.1, 0.05, 0.3, 1}}) {
            const auto a = pixel(row_times(v, p), w, h), b = pixel(row_times(v, *j), w, h);
            CHECK(near(b[0] - a[0], s.x, 2e-4) && near(b[1] - a[1], s.y, 2e-4));
            CHECK(near(row_times(v, *j)[2], row_times(v, p)[2], 0) && near(row_times(v, *j)[3], row_times(v, p)[3], 0));   // depth untouched
        }
    }
    // An orthographic matrix (_34 = 0, _44 = 1) is shifted through its last row: the general row-vector rule.
    const EngineMatrix ortho{0.01f, 0, 0, 0, 0, 0.02f, 0, 0, 0, 0, 0.001f, 0, 0.1f, -0.2f, 0.5f, 1};
    const auto o = jitter_engine_projection(ortho, 0.25f, -0.125f, 200, 100);
    CHECK(o && (*o)[8] == 0.0f && (*o)[9] == 0.0f && near((*o)[12], 0.1 + 2 * 0.25 / 200, 1e-7) && near((*o)[13], -0.2 - 2 * -0.125 / 100, 1e-7));
    const auto a = pixel(row_times({3, 4, 10, 1}, ortho), 200, 100), b = pixel(row_times({3, 4, 10, 1}, *o), 200, 100);
    CHECK(near(b[0] - a[0], 0.25, 1e-4) && near(b[1] - a[1], -0.125, 1e-4));
    // Same values as temporal::jitter_projection on the column-vector transpose (the existing helper, reused).
    const auto t = temporal::jitter_projection(to_column(p), {double(0.3f), double(-0.2f)}, {w, h});
    CHECK(t && from_column(*t) == *jitter_engine_projection(p, 0.3f, -0.2f, w, h));
    // Refusals: non-finite matrix or jitter, degenerate extent.
    auto bad = p;
    bad[5] = std::numeric_limits<float>::infinity();
    CHECK(!jitter_engine_projection(bad, 0.1f, 0.1f, w, h));
    CHECK(!jitter_engine_projection(p, std::numeric_limits<float>::quiet_NaN(), 0.1f, w, h));
    CHECK(!jitter_engine_projection(p, 0.1f, 0.1f, 1, h) && !jitter_engine_projection(p, 0.1f, 0.1f, w, 0));
    // Fmatrix::mul(P, V): v * mFullTransform == (v * V) * P.
    const EngineMatrix view{0.8f, 0, -0.6f, 0, 0, 1, 0, 0, 0.6f, 0, 0.8f, 0, -3, -1.5f, 7.25f, 1};
    const auto full = engine_mul(p, view);
    for (const auto& v : {std::array<double, 4>{1, 2, 3, 1}, {-5, 0.5, 20, 1}}) {
        const auto direct = row_times(v, full), chained = row_times(row_times(v, view), p);
        for (int i = 0; i < 4; ++i) CHECK(near(direct[i], chained[i], 1e-4 * (1 + std::fabs(chained[i]))));
    }
}

void first_person_rule() {
    const auto main = *jitter_engine_projection(perspective(1.2f, 1.7778f), 0.25f, -0.1f, 1286, 724);
    const auto fresh = perspective(0.9f, 1.7778f, 0.05f);
    const auto fp = *jitter_engine_projection(fresh, 0.25f, -0.1f, 1286, 724);
    // A freshly built first-person projection is jittered; the jittered main projection and the last first-person
    // matrix the adapter wrote are never jittered again (rmNear without a rebuild, a second rmNear at the same site).
    CHECK(first_person_action(fresh, main, nullptr) == FirstPersonAction::Jitter);
    CHECK(first_person_action(main, main, nullptr) == FirstPersonAction::AlreadyJittered);
    CHECK(first_person_action(fp, main, &fp) == FirstPersonAction::AlreadyJittered);
    // Without the record of what was written, a jittered first-person matrix has _31/_32 != 0: left alone, not jittered.
    CHECK(first_person_action(fp, main, nullptr) == FirstPersonAction::Foreign);
    // Applying the jitter twice would double the offset: the rule prevents that, and a single application is exact.
    const auto twice = *jitter_engine_projection(fp, 0.25f, -0.1f, 1286, 724);
    CHECK(near(twice[8], 2 * fp[8], 1e-8) && first_person_action(twice, main, &fp) == FirstPersonAction::Foreign);
    // A zero sample leaves _31/_32 at zero: the fresh matrix is "jittered" by nothing, which is harmless.
    const auto zero = *jitter_engine_projection(fresh, 0.0f, 0.0f, 1286, 724);
    CHECK(zero == fresh);
}

void mip_bias() {
    CHECK(near(window_mip_bias_delta(1286, 1920, -0.3f), std::log2(1286.0 / 1920.0) - 0.3, 1e-6));
    CHECK(near(window_mip_bias_delta(960, 1920, 0.0f), -1.0, 1e-7) && window_mip_bias_delta(1920, 1920, 0.0f) == 0.0f);
    CHECK(near(window_mip_bias_delta(1920, 1920, -0.3f), -0.3, 1e-7));
    CHECK(window_mip_bias_delta(960, 1920, -5.0f) == -3.0f);                       // offset clamped to -2, total -3
    CHECK(window_mip_bias_delta(1920, 1920, 5.0f) == 1.0f);                        // offset clamped to +1
    CHECK(near(window_mip_bias_delta(1286, 1920, std::numeric_limits<float>::quiet_NaN()), std::log2(1286.0 / 1920.0) - 0.3, 1e-6));
    CHECK(window_mip_bias_delta(0, 1920, 0.0f) == 0.0f && window_mip_bias_delta(1286, 0, 0.0f) == 0.0f);
    CHECK(clamp_mip_bias_offset(-0.75f) == -0.75f && clamp_mip_bias_offset(-9.0f) == -2.0f && clamp_mip_bias_offset(3.0f) == 1.0f);
}

void published_state() {
    JitterState main{};
    const auto s0 = jitter_sample(0, 18), s1 = jitter_sample(1, 18), s2 = jitter_sample(2, 18);
    main = next_state(main, false, true, s0, 1286, 724, true);
    CHECK(main.frame == 1 && main.applied && !main.previous_applied && main.reset && main.current_x == s0.x);
    auto uv = jitter_uv(main);
    CHECK(near(uv.current_u, s0.x / 1286.0, 1e-7) && near(uv.current_v, s0.y / 724.0, 1e-7) && uv.previous_u == 0 && uv.previous_v == 0);
    main = next_state(main, false, true, s1, 1286, 724, false);
    uv = jitter_uv(main);
    CHECK(main.frame == 2 && main.previous_applied && main.previous_x == s0.x && main.previous_y == s0.y);
    CHECK(near(uv.previous_u, s0.x / 1286.0, 1e-7) && near(uv.current_u, s1.x / 1286.0, 1e-7));
    // A scope frame in between publishes zeros (its history is separate and unjittered) and does not disturb the main
    // chain: the next main frame's previous jitter is still the last main frame's.
    const auto scope = next_state(main, true, false, s2, 1286, 724, false);
    uv = jitter_uv(scope);
    CHECK(scope.scope && !scope.applied && uv.current_u == 0 && uv.current_v == 0 && uv.previous_u == 0 && uv.previous_v == 0);
    main = next_state(main, false, true, s2, 1286, 724, false);
    CHECK(main.previous_x == s1.x && main.frame == 3 && main.sequence_index == 2 && main.phase_count == 18);
    // A main frame whose window did not open (allocation failure, MSAA): current zero, previous still the real one.
    main = next_state(main, false, false, jitter_sample(3, 18), 1286, 724, false);
    uv = jitter_uv(main);
    CHECK(!main.applied && uv.current_u == 0 && near(uv.previous_u, s2.x / 1286.0, 1e-7));
    main = next_state(main, false, true, jitter_sample(4, 18), 1286, 724, false);
    CHECK(!main.previous_applied && jitter_uv(main).previous_u == 0);
    // The previous jitter is expressed in the previous frame's render size.
    main = next_state(main, false, true, jitter_sample(0, 32), 960, 540, true);
    uv = jitter_uv(main);
    CHECK(near(uv.previous_u, jitter_sample(4, 18).x / 1286.0, 1e-7) && near(uv.current_u, jitter_sample(0, 32).x / 960.0, 1e-7));
}

// The motion PS (shaders/core/rsf_core_pixel.h) on the CPU: current UV from the rasterized (jittered) position minus
// the current jitter UV, previous UV from the previous jittered clip minus the previous jitter UV.
void motion_cancellation() {
    const uint32_t w = 858, h = 482;
    const auto p = perspective(1.2f, 1280.0f / 720.0f);
    const EngineMatrix view{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.5f, -1, 3, 1};
    JitterSequence sequence;
    JitterState state{};
    EngineMatrix previous_full{};
    int checked = 0;
    for (int frame = 0; frame < 20; ++frame) {
        const auto sample = sequence.next({w, h, 1280, 720, 0}, true);
        state = next_state(state, false, true, sample, w, h, sequence.last_was_reset());
        const auto full = engine_mul(*jitter_engine_projection(p, sample.x, sample.y, w, h), view);
        if (frame > 0) {
            const auto uv = jitter_uv(state);
            for (const auto& v : {std::array<double, 4>{0, 0, 5, 1}, {-2, 1.5, 12, 1}, {4, -3, 60, 1}}) {
                const auto current = pixel(row_times(v, full), w, h);
                const auto prev = row_times(v, previous_full);
                const double cu = current[0] / w - uv.current_u, cv = current[1] / h - uv.current_v;
                const double pu = prev[0] / prev[3] * 0.5 + 0.5 - uv.previous_u, pv = 0.5 - prev[1] / prev[3] * 0.5 - uv.previous_v;
                CHECK(near(cu - pu, 0, 2e-6) && near(cv - pv, 0, 2e-6));
                // Without the cancellation the static scene would show the jitter difference as motion.
                CHECK(near(current[0] / w - (prev[0] / prev[3] * 0.5 + 0.5), uv.current_u - uv.previous_u, 2e-6));
                ++checked;
            }
        }
        previous_full = full;
    }
    CHECK(checked == 57);
}
// The wave 6 failure: a burst started while the "Press any key to continue" screen was up (the world renders under it, the
// frames reach H1, frames_without_world does not move, only the engine's loading-screen flag is set).
void burst_gate() {
    BurstGate gate;
    uint64_t no_world = 7;
    // The very first frame has nothing to compare with and is held; the loading screen holds for any length.
    CHECK(!gate.frame(true, no_world, false).capture);
    for (int frame = 0; frame < 1200; ++frame) {
        const auto verdict = gate.frame(true, no_world, false);
        CHECK(!verdict.capture && !verdict.discard);
    }
    CHECK(gate.clear_frames() == 0 && gate.restarts() == 0);
    // After dismissal the burst waits for the settle frames: 29 clear frames hold, the 30th starts it.
    for (uint32_t frame = 1; frame < BurstGate::default_settle_frames; ++frame) CHECK(!gate.frame(false, no_world, false).capture);
    CHECK(gate.clear_frames() == BurstGate::default_settle_frames - 1);
    CHECK(gate.frame(false, no_world, false).capture);
    // Once frames are taken it continues without waiting.
    for (int frame = 0; frame < 5; ++frame) { const auto verdict = gate.frame(false, no_world, true); CHECK(verdict.capture && !verdict.discard); }
    // A menu or loading frame (a frame that never reached H1) in the middle discards the frames and starts over.
    ++no_world;
    auto verdict = gate.frame(false, no_world, true);
    CHECK(!verdict.capture && verdict.discard && gate.restarts() == 1 && gate.clear_frames() == 0);
    for (uint32_t frame = 1; frame < BurstGate::default_settle_frames; ++frame) CHECK(!gate.frame(false, no_world, false).capture);
    CHECK(gate.frame(false, no_world, false).capture);
    // So does the loading screen appearing mid-burst, and one seen only just before Present.
    verdict = gate.frame(true, no_world, true);
    CHECK(!verdict.capture && verdict.discard && gate.restarts() == 2);
    for (uint32_t frame = 0; frame < BurstGate::default_settle_frames; ++frame) gate.frame(false, no_world, false);
    gate.interrupted();
    CHECK(gate.restarts() == 3 && gate.clear_frames() == 0 && !gate.frame(false, no_world, false).capture);
    // A held frame with nothing taken is not a restart; a custom settle of 1 starts on the first clear frame.
    BurstGate quick(1);
    CHECK(!quick.frame(false, 0, false).capture && quick.frame(false, 0, false).capture && quick.restarts() == 0);
}
}

int main(int argc, char** argv) {
    struct { const char* name; void (*run)(); } cases[] = {{"halton", halton_sequence}, {"sequence", sequence_resets},
        {"ndc", ndc_offset}, {"first_person", first_person_rule}, {"mip_bias", mip_bias}, {"published", published_state},
        {"motion_cancellation", motion_cancellation}, {"burst_gate", burst_gate}};
    int ran = 0;
    for (const auto& item : cases) {
        if (argc > 1 && std::string_view{argv[1]} != item.name) continue;
        item.run();
        ++ran;
    }
    if (!ran) { std::printf("unknown case\n"); return 2; }
    std::printf("%d case(s), %d failure(s)\n", ran, failures);
    return failures ? 1 : 0;
}
