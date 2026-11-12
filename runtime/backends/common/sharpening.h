// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <algorithm>
#include <cmath>
namespace rsf {
// Both fields are overwritten on every dispatch, including Off and custom modes.
template<class Dispatch> void ffx_sharpening(Dispatch& dispatch, float sharpness) {
    const float amount = std::isfinite(sharpness) ? std::clamp(sharpness,0.0f,1.0f) : 0.0f;
    dispatch.enableSharpening = amount > 0;
    dispatch.sharpness = amount;
}
}
