// SPDX-License-Identifier: MIT
#include "anomaly_temporal_oracle.h"

extern "C" int32_t rsf_anomaly_temporal_planning_fixture(const char*, const rsf_anomaly_plan_fixture*) noexcept;

int main(int argc, char** argv) {
    return rsf_anomaly_temporal_planning_fixture(argc > 1 ? argv[1] : nullptr,
        &rsf::anomaly::temporal::fixture::oracle);
}
