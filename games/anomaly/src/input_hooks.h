// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// CInput calls a native function that may unwind through C++. Keep its invocation
// inside a native scope so the in-flight ticket is retired during that unwind.
#ifdef __cplusplus
#define RSF_INPUT_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_INPUT_NOEXCEPT
#endif

typedef bool (*rsf_anomaly_input_read)(void*, const void*, size_t) RSF_INPUT_NOEXCEPT;
typedef bool (*rsf_anomaly_input_exchange)(void*, void*, void*, void**) RSF_INPUT_NOEXCEPT;

uint32_t rsf_anomaly_input_frame_begin(rsf_anomaly_input_read, void (*)(uint32_t) RSF_INPUT_NOEXCEPT) RSF_INPUT_NOEXCEPT;
void rsf_anomaly_input_frame_exit(uint32_t) RSF_INPUT_NOEXCEPT;

#ifdef __cplusplus
}

#endif
#undef RSF_INPUT_NOEXCEPT
