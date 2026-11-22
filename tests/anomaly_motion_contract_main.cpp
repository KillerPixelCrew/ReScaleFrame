// SPDX-License-Identifier: MIT
#include "shader_frame_layout.h"

extern "C" int rsf_anomaly_motion_contract(const char*, const rsf_anomaly_shader_frame_layout*,
    const rsf_anomaly_shader_frame_layout*) noexcept;

int main(int argc, char** argv)
{
    using namespace rsf::anomaly::motion;
    const auto standard = shader_frame_layout(motion_standard);
    const auto avx = shader_frame_layout(motion_avx);
    return rsf_anomaly_motion_contract(argc > 1 ? argv[1] : nullptr, &standard, &avx);
}
