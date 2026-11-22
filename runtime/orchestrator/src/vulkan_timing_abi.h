// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
int64_t rsf_core_vk_timing_begin(void);
void rsf_core_vk_timing_end(const char* stage, uint64_t frame, int64_t start);
#ifdef __cplusplus
}
#endif
