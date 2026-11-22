// SPDX-License-Identifier: MIT
#include <rescaleframe/overlay_api.h>

extern "C" int rsf_test_overlay_no_sr(rsf_get_overlay_api_fn);

int main()
{
    return rsf_test_overlay_no_sr(rsf_get_overlay_api);
}
