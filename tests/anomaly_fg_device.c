// SPDX-License-Identifier: MIT
// The Rust fixture owns the CPU and device scenarios and the input profile facts.
#include "fg_stage/fixture_platform.h"
#include <rescaleframe/d3d11_state.h>
#include <stdio.h>

int rsf_test_anomaly_fg(int argc, const char* const* argv, const rsf_anomaly_fg_fixture_platform*);

int main(int argc, char** argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    const rsf_anomaly_fg_fixture_platform platform = {
        sizeof(rsf_anomaly_fg_fixture_platform),
        {rsf_d3d11_state_save, rsf_d3d11_state_restore},
        rsf_native_cpu_release, rsf_native_cpu_ping_message, rsf_native_cpu_read, rsf_native_cpu_counters_get,
    };
    return rsf_test_anomaly_fg(argc, (const char* const*)argv, &platform);
}
