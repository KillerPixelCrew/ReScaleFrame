/* SPDX-License-Identifier: MIT */
#include <rescaleframe/overlay_api.h>

#ifdef RSF_RUNTIME_BUILD
#error The fixture must import Runtime.
#endif

int rsf_test_overlay_device(rsf_get_overlay_api_fn query);

int main(void)
{
    return rsf_test_overlay_device(rsf_get_overlay_api);
}
