/* SPDX-License-Identifier: MIT */
/* Synthetic concrete providers test dispatch and admission without GPU or SDK calls. */
#include "dispatch_abi.h"

int main(void) {
#if defined(RSF_HAVE_FSR4_FG_VK)
    return rsf_fixture_vulkan_dispatch_run(true, false);
#elif defined(RSF_HAVE_XESS_FG_VK)
    return rsf_fixture_vulkan_dispatch_run(false, true);
#else
    return rsf_fixture_vulkan_dispatch_run(false, false);
#endif
}
