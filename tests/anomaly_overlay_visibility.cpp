// SPDX-License-Identifier: MIT
#include <rescaleframe/overlay_api.h>

extern "C" int rsf_anomaly_overlay_visibility_fixture(rsf_get_overlay_api_fn) noexcept;

int main() {
    return rsf_anomaly_overlay_visibility_fixture(&rsf_get_overlay_api);
}
