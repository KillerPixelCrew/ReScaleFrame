/* SPDX-License-Identifier: MIT */
/* The swap owner's fixtures on the production engine layout (sr_profile.rs), compared with an independent copy. */
#include "sr_swap.h"
#include <stdint.h>

int32_t rsf_anomaly_sr_swap_fixture(int, char* const*, const rsf_anomaly_swap_layout*);

int main(int argc, char** argv) {
    return rsf_anomaly_sr_swap_fixture(argc, argv, rsf_anomaly_sr_engine_layout());
}