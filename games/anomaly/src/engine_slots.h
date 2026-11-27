// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>

enum { RSF_ANOMALY_INPUT_SLOT = 0, RSF_ANOMALY_POSITION_SLOT = 1 };

#ifdef __cplusplus
#define RSF_ANOMALY_SLOT_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_ANOMALY_SLOT_NOEXCEPT
#endif

bool rsf_anomaly_compare_slot(void*, void*, void*, void**) RSF_ANOMALY_SLOT_NOEXCEPT;
bool rsf_anomaly_slot_set(uint32_t kind, bool enabled) RSF_ANOMALY_SLOT_NOEXCEPT;
void* rsf_anomaly_slot_original(uint32_t kind) RSF_ANOMALY_SLOT_NOEXCEPT;
bool rsf_anomaly_position_slot_active(void) RSF_ANOMALY_SLOT_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_ANOMALY_SLOT_NOEXCEPT
