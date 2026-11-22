// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>

/* Both outputs are required and overwritten on every dispatch. */
#ifdef __cplusplus
extern "C" {
#endif
void rsf_ffx_sharpening(float request, bool* enabled, float* amount);
#ifdef __cplusplus
}
#endif
