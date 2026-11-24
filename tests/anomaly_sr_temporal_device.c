/* SPDX-License-Identifier: MIT */
#include "sr_d3d11/temporal_device_platform.h"

int32_t rsf_anomaly_sr_temporal_device_fixture(const rsf_anomaly_temporal_fixture_graphics*, int, char* const*);

int main(int argc, char** argv) {
    static const rsf_anomaly_temporal_fixture_graphics graphics = {
        rsf_motion_decode_create, rsf_motion_decode_run, rsf_motion_decode_texture, rsf_motion_decode_destroy,
        rsf_motion_resolve_create, rsf_motion_resolve_run, rsf_motion_resolve_motion, rsf_motion_resolve_destroy,
    };
    return rsf_anomaly_sr_temporal_device_fixture(&graphics, argc, argv);
}
