// SPDX-License-Identifier: MIT
#include "sr_d3d11_platform.h"
#include "sr_d3d11/temporal_device_platform.h"

namespace rsf::anomaly::sr {
const rsf_sr_d3d11& d3d11_platform() noexcept;
}

extern "C" int32_t rsf_anomaly_sr_temporal_device_fixture(const rsf_sr_d3d11*,
    const rsf_anomaly_temporal_fixture_graphics*, const char*, const char*) noexcept;

int main(int argc, char** argv) {
    static constexpr rsf_anomaly_temporal_fixture_graphics graphics{
        rsf_motion_decode_create, rsf_motion_decode_run, rsf_motion_decode_texture, rsf_motion_decode_destroy,
        rsf_motion_resolve_create, rsf_motion_resolve_run, rsf_motion_resolve_motion, rsf_motion_resolve_destroy,
    };
    return rsf_anomaly_sr_temporal_device_fixture(&rsf::anomaly::sr::d3d11_platform(), &graphics,
        argc > 1 ? argv[1] : nullptr, argc > 2 ? argv[2] : nullptr);
}
