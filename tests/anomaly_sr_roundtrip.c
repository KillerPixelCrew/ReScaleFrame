/* SPDX-License-Identifier: MIT */
#include <stdint.h>

struct rsf_sr_d3d11;
const struct rsf_sr_d3d11* rsf_anomaly_sr_d3d11_platform(void);
int32_t rsf_anomaly_sr_roundtrip_device_fixture(const struct rsf_sr_d3d11*);

int main(void) {
    return rsf_anomaly_sr_roundtrip_device_fixture(rsf_anomaly_sr_d3d11_platform());
}
