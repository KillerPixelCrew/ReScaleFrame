// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// CPU reference of the Anomaly "low" tonemap curve and its exact inverse (docs/research/anomaly-sr-seam.md 2.2).
// The GPU pass in shaders/sr/rsf_sr_roundtrip.hlsl implements the same formulas; the D3D11 fixture compares both.
#include <cmath>

namespace rsf::anomaly::sr {
inline constexpr double tonemap_white = 1.7;
inline constexpr double tonemap_white_squared = tonemap_white * tonemap_white;
// f(x) = x(1 + x/W^2)/(1 + x). Reaches 1 at x = W.
inline double tonemap_forward(double x) noexcept { return x * (1.0 + x / tonemap_white_squared) / (1.0 + x); }
// f^-1(y), cancellation-free on both sides of y = 1.
inline double tonemap_inverse(double y) noexcept {
    const double b = 1.0 - y;
    const double d = std::sqrt(b * b + 4.0 * y / tonemap_white_squared);
    return b > 0 ? 2.0 * y / (b + d) : 0.5 * tonemap_white_squared * (d - b);
}
// What a stock UNORM8 render target stores for a shader output y (D3D float -> UNORM conversion: saturate, NaN -> 0,
// then round to nearest).
inline int unorm8(double y) noexcept {
    if (!(y > 0)) return 0;
    if (y >= 1) return 255;
    return static_cast<int>(std::floor(y * 255.0 + 0.5));
}
}
