// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <windows.h>

#ifdef __cplusplus
#define RSF_ANOMALY_RUNTIME_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_ANOMALY_RUNTIME_NOEXCEPT
#endif

// The carrier owns the Runtime reference. These calls only borrow it.
HMODULE rsf_anomaly_runtime_module(void) RSF_ANOMALY_RUNTIME_NOEXCEPT;
void rsf_anomaly_runtime_set_colour_transport(bool) RSF_ANOMALY_RUNTIME_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_ANOMALY_RUNTIME_NOEXCEPT
