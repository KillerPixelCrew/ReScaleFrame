/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The caller owns the event. The result is the raw HRESULT. */
int32_t rsf_core_d3d12_wait_native_fence(void* fence, uint64_t value, void* event, uint32_t timeout_ms);

#ifdef __cplusplus
}
#endif
