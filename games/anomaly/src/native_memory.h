// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <cstring>

namespace rsf::anomaly {
inline bool guarded_copy(void* output, const void* input, std::size_t size) noexcept {
    if (!output || !input) return false;
#ifdef _MSC_VER
    __try {
#endif
        std::memcpy(output, input, size);
        return true;
#ifdef _MSC_VER
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}
}
