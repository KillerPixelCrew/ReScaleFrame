/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_fixture_sl_find_adapter(uint32_t physical, const uint8_t*, uint32_t length, uint32_t mask);
uint32_t rsf_fixture_sl_selected_mask(const uint8_t*, uint32_t length, uint32_t has_lookup);
uint32_t rsf_fixture_sl_config_applies(uint32_t supported, uint32_t selected);
uint32_t rsf_fixture_sl_device_config_applies(uint32_t feature, uint32_t supported, uint32_t selected, uint32_t vendor);
int32_t rsf_fixture_sl_selected_run(void);

void rsf_fixture_sl_dispatch_configure(uintptr_t owner, uintptr_t resolve);
uintptr_t rsf_fixture_sl_dispatch_get(uintptr_t device, const char* name);
uintptr_t rsf_fixture_sl_dispatch_fill(void);
void rsf_fixture_sl_dispatch_fresh(uintptr_t image, uint32_t mips, uint32_t layers, uint32_t aspects);
uint32_t rsf_fixture_sl_dispatch_tracked(uintptr_t image);
/* VkImageMemoryBarrier records. */
uint32_t rsf_fixture_sl_dispatch_barriers(const void* requested, void* output, uint32_t capacity);
int32_t rsf_fixture_sl_dispatch_run(void);
#ifdef __cplusplus
}
#endif
