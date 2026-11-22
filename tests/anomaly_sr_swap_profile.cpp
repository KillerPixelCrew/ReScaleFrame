// SPDX-License-Identifier: MIT
#include "sr_swap_layout.h"

extern "C" const rsf_anomaly_swap_layout* rsf_test_anomaly_swap_layout(void) noexcept {
    return &rsf::anomaly::sr::verified_layout;
}
