// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// Optional in-process diagnostic callbacks. Register before census installation; callback code
// must remain loaded until exit, must not throw, and receives borrowed COM pointers only.
void rsf_census_observe_d3d12(
    void (*device_created)(void*),
    void (*before_submit)(void*, uint32_t, void* const*),
    void (*after_submit)(void*, uint32_t, void* const*),
    void (*before_present)(void*),
    void (*swapchain_created)(void*),
    void (*color_space_set)(void*, uint32_t));
// Successful SR creation, evaluation before/after the original call, and successful release.
// Evaluation phase is 0 before and 1 after; result is meaningful only after the original call.
void rsf_census_observe_ngx_d3d12(
    void (*created)(void* handle, const void* parameters, uint32_t feature),
    void (*evaluate)(void* command_list, void* handle, const void* parameters, uint32_t phase, uint64_t result),
    void (*released)(void* handle));
#ifdef __cplusplus
}
#endif
