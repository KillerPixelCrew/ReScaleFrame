/* SPDX-License-Identifier: MIT */
#include <stdint.h>
#include <stddef.h>

int32_t rsf_anomaly_temporal_planning_fixture(const char*);

int main(int argc, char** argv) {
    return rsf_anomaly_temporal_planning_fixture(argc > 1 ? argv[1] : NULL);
}
