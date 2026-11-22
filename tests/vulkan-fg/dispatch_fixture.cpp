// SPDX-License-Identifier: MIT
// Synthetic concrete providers test dispatch/admission only, without GPU/SDKs.
#include "../../runtime/orchestrator/src/native_fg_vulkan_dispatch.cpp"
#include "dispatch_abi.h"

int main() {
#if defined(RSF_HAVE_FSR4_FG_VK)
    return rsf_fixture_vulkan_dispatch_run(true, false);
#elif defined(RSF_HAVE_XESS_FG_VK)
    return rsf_fixture_vulkan_dispatch_run(false, true);
#else
    return rsf_fixture_vulkan_dispatch_run(false, false);
#endif
}
