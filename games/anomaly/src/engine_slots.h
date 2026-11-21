// SPDX-License-Identifier: MIT
#pragma once
#include "engine_guards.h"

enum : uint32_t { RSF_ANOMALY_INPUT_SLOT = 0, RSF_ANOMALY_POSITION_SLOT = 1 };
using rsf_anomaly_slot_exchange = bool (*)(void*, void*, void*, void**) noexcept;

extern "C" {
bool rsf_anomaly_slot_install(uint32_t kind, const rsf_anomaly_guard_image*, uint32_t slot_rva, uint32_t function_rva,
    uint32_t counter_rva, void* replacement, rsf_anomaly_slot_exchange) noexcept;
bool rsf_anomaly_slot_set(uint32_t kind, bool enabled) noexcept;
void* rsf_anomaly_slot_original(uint32_t kind) noexcept;
bool rsf_anomaly_position_slot_active() noexcept;
}

namespace rsf::anomaly {
inline bool compare_slot(void* address, void* expected, void* value, void** previous) noexcept {
#ifdef _MSC_VER
    __try {
#endif
        *previous = InterlockedCompareExchangePointer(static_cast<void* volatile*>(address), value, expected);
        return true;
#ifdef _MSC_VER
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}
}
