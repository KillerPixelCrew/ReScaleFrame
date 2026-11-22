/* SPDX-License-Identifier: MIT */
/* Motion contract cases (motion/fixture.rs); the caller-frame layouts come from motion_profile.rs. */
#include <stdint.h>

int32_t rsf_anomaly_motion_contract(const char* selection);

int main(int argc, char** argv) {
    return rsf_anomaly_motion_contract(argc > 1 ? argv[1] : 0);
}
