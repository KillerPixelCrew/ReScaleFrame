// SPDX-License-Identifier: MIT
#include <rescaleframe/overlay_api.h>

int rsf_anomaly_overlay_visibility_fixture(rsf_get_overlay_api_fn);

int main(void) {
    return rsf_anomaly_overlay_visibility_fixture(&rsf_get_overlay_api);
}
