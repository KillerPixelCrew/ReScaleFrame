// SPDX-License-Identifier: MIT
// Import the real Runtime as a DLL consumer. The Rust fixture calls this entry
// for negotiation and idle checks and owns the synthetic Runtime cases.
#include <rescaleframe/d3d11_sr_api.h>
#include <stddef.h>

int32_t rsf_anomaly_sr_runtime_fixture(const char*, rsf_get_d3d11_sr_api_fn);

int main(int argc, char** argv) {
    return rsf_anomaly_sr_runtime_fixture(argc > 1 ? argv[1] : NULL, &rsf_get_d3d11_sr_api);
}
