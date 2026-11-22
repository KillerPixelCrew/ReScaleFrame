// SPDX-License-Identifier: MIT
// Let the native compiler lay out the NGX parameter object's virtual functions.
#include "../vendor_census_ngx_params.h"

namespace {
FakeNgxParams parameters;
}

extern "C" int NVSDK_NGX_D3D12_GetParameters(NgxParam** out) {
    *out = &parameters;
    return 1;
}
