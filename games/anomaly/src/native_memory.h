// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <stdbool.h>
#include <string.h>
#ifdef __cplusplus
#include <cstring>
namespace rsf::anomaly {
inline bool guarded_copy(void* output, const void* input, size_t size) noexcept
#else
static inline bool guarded_copy(void* output, const void* input, size_t size)
#endif
{
    if (!output || !input) return false;
    if (!size) return true;
#ifdef _MSC_VER
    __try {
        memcpy(output, input, size);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    const DWORD error = GetLastError();
    SIZE_T copied = 0;
    const bool complete = ReadProcessMemory(GetCurrentProcess(), input, output, size, &copied) && copied == size;
    SetLastError(error);
    return complete;
#endif
}

#ifdef __cplusplus
}
#endif
