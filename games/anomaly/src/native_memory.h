// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <cstring>

namespace rsf::anomaly {
inline bool guarded_copy(void* output, const void* input, std::size_t size) noexcept {
    if (!output || !input) return false;
    if (!size) return true;
#ifdef _MSC_VER
    __try {
        std::memcpy(output, input, size);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    const auto error = GetLastError();
    SIZE_T copied{};
    const bool complete = ReadProcessMemory(GetCurrentProcess(), input, output, size, &copied) && copied == size;
    SetLastError(error);
    return complete;
#endif
}
}
