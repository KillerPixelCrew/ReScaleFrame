/* SPDX-License-Identifier: MIT */
#include <stdint.h>

/* These executable exports are consumed by the hybrid GPU driver before main. */
__declspec(dllexport) uint32_t NvOptimusEnablement = 1;
__declspec(dllexport) int AmdPowerXpressRequestHighPerformance = 1;

int rsf_gl_shared_surface_fixture(void);

int main(void) {
    return rsf_gl_shared_surface_fixture();
}
