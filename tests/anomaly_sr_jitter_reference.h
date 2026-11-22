// SPDX-License-Identifier: MIT
#pragma once
// Independent column-vector and Fmatrix oracles for the native fixtures.
#include "../games/anomaly/src/sr_jitter.h"
#include "../games/anomaly/temporal/math.h"
#include <cmath>

namespace rsf::anomaly::sr {
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
}
