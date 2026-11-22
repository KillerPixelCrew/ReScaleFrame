// SPDX-License-Identifier: MIT
#include "anomaly_sr_jitter_reference.h"
#include "sr_swap_layout.h"

using rsf::anomaly::sr::EngineMatrix;
using CameraOracle = bool (*)(const EngineMatrix*, const EngineMatrix*, float, float, uint32_t, uint32_t,
    EngineMatrix*, EngineMatrix*) noexcept;
extern "C" int32_t rsf_anomaly_sr_swap_fixture(const char*, const rsf_anomaly_swap_layout*, CameraOracle) noexcept;

namespace {
bool camera_oracle(const EngineMatrix* projection, const EngineMatrix* view, float x, float y, uint32_t width,
    uint32_t height, EngineMatrix* jittered, EngineMatrix* full) noexcept {
    using namespace rsf::anomaly::sr;
    const auto result = jitter_engine_projection(*projection, x, y, width, height);
    if (!result) return false;
    *jittered = *result;
    *full = engine_mul(*jittered, *view);
    return true;
}
}

int main(int argc, char** argv) {
    return rsf_anomaly_sr_swap_fixture(argc > 1 ? argv[1] : nullptr,
        &rsf::anomaly::sr::verified_layout, camera_oracle);
}
