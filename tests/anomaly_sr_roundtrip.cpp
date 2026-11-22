// SPDX-License-Identifier: MIT
#include "sr_d3d11_platform.h"

namespace rsf::anomaly::sr {
const rsf_sr_d3d11& d3d11_platform() noexcept;
}

extern "C" int32_t rsf_anomaly_sr_roundtrip_device_fixture(const rsf_sr_d3d11*) noexcept;

int main() {
    return rsf_anomaly_sr_roundtrip_device_fixture(&rsf::anomaly::sr::d3d11_platform());
}
