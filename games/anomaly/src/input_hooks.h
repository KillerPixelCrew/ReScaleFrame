// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// CInput calls a native function that may unwind through C++. Keep its invocation
// inside a native scope so the in-flight ticket is retired during that unwind.
#ifdef __cplusplus
#include "engine_slots.h"
#define RSF_INPUT_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_INPUT_NOEXCEPT
#endif

struct rsf_anomaly_guard_image;
typedef bool (*rsf_anomaly_input_read)(void*, const void*, size_t) RSF_INPUT_NOEXCEPT;
typedef bool (*rsf_anomaly_input_exchange)(void*, void*, void*, void**) RSF_INPUT_NOEXCEPT;

bool rsf_anomaly_input_install(const struct rsf_anomaly_guard_image*, size_t build,
    uint32_t, void*, rsf_anomaly_input_exchange, const char**) RSF_INPUT_NOEXCEPT;
uint32_t rsf_anomaly_input_frame_begin(rsf_anomaly_input_read, void (*)(uint32_t) RSF_INPUT_NOEXCEPT) RSF_INPUT_NOEXCEPT;
void rsf_anomaly_input_frame_exit(uint32_t) RSF_INPUT_NOEXCEPT;
bool rsf_anomaly_input_idle(void) RSF_INPUT_NOEXCEPT;
void rsf_anomaly_input_received(uint8_t, void (*)(uint32_t, uint32_t) RSF_INPUT_NOEXCEPT) RSF_INPUT_NOEXCEPT;
void rsf_anomaly_overlay_set_input_observer(void (*)(uint8_t) RSF_INPUT_NOEXCEPT) RSF_INPUT_NOEXCEPT;

#ifdef __cplusplus
}

namespace rsf::anomaly::input {
inline bool idle() noexcept { return rsf_anomaly_input_idle(); }
inline bool enable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, true); }
inline bool disable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, false); }
}
#endif
#undef RSF_INPUT_NOEXCEPT
