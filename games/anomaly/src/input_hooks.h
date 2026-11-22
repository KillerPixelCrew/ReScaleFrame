// SPDX-License-Identifier: MIT
#pragma once
#include "engine_slots.h"

// CInput calls a native function that may unwind through C++. Keep its invocation
// inside a native scope so the in-flight ticket is retired during that unwind.
namespace rsf::anomaly::fg { void input_begin(uint32_t) noexcept; void input_received(uint32_t, uint32_t) noexcept; }

extern "C" {
bool rsf_anomaly_input_install(const rsf_anomaly_guard_image*, size_t build,
    uint32_t, void*, rsf_anomaly_slot_exchange, const char**) noexcept;
uint32_t rsf_anomaly_input_frame_begin(decltype(&rsf::anomaly::guarded_copy), void (*)(uint32_t) noexcept) noexcept;
void rsf_anomaly_input_frame_exit(uint32_t) noexcept;
bool rsf_anomaly_input_idle() noexcept;
void rsf_anomaly_input_received(uint8_t, void (*)(uint32_t, uint32_t) noexcept) noexcept;
void rsf_anomaly_overlay_set_input_observer(void (*)(uint8_t) noexcept) noexcept;
}

namespace rsf::anomaly::input {
inline bool idle() noexcept { return rsf_anomaly_input_idle(); }
inline bool enable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, true); }
inline bool disable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, false); }
}
