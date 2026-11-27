// SPDX-License-Identifier: MIT
// The guarded engine-memory copy.
#pragma once
#include <windows.h>
#include <stdbool.h>
#include <string.h>
#include <rescaleframe/foreign_guard.h>

static inline bool guarded_copy(void* output, const void* input, size_t size)
{
    if (!output || !input) return false;
    if (!size) return true;
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
        memcpy(output, input, size);
        return true;
    } RSF_FAULT_CATCH { return false; }
#else
    const DWORD error = GetLastError();
    SIZE_T copied = 0;
    const bool complete = ReadProcessMemory(GetCurrentProcess(), input, output, size, &copied) && copied == size;
    SetLastError(error);
    return complete;
#endif
}
