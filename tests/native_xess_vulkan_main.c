// SPDX-License-Identifier: MIT
#include <stdbool.h>

int rsf_fixture_xess_sr_main(int argc, bool device_available);

int main(int argc, char** argv) {
    (void)argv;
#if defined(RSF_TEST_VULKAN)
    return rsf_fixture_xess_sr_main(argc, true);
#else
    return rsf_fixture_xess_sr_main(argc, false);
#endif
}
