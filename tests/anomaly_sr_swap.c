/* SPDX-License-Identifier: MIT */
#include <stdint.h>

struct rsf_anomaly_swap_layout;
const struct rsf_anomaly_swap_layout* rsf_test_anomaly_swap_layout(void);
int32_t rsf_anomaly_sr_swap_fixture(int, char* const*, const struct rsf_anomaly_swap_layout*);

int main(int argc, char** argv) {
    return rsf_anomaly_sr_swap_fixture(argc, argv, rsf_test_anomaly_swap_layout());
}
