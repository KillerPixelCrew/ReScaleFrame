/* SPDX-License-Identifier: MIT */
#include <stddef.h>

int rsf_anomaly_sr_jitter_fixture(const char* selection);

int main(int argc, char** argv) {
    return rsf_anomaly_sr_jitter_fixture(argc > 1 ? argv[1] : NULL);
}
