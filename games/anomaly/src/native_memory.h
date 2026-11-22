// SPDX-License-Identifier: MIT
// The guarded engine-memory copy and the void form of the shared guarded host log (foreign_guard.h).
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

static inline void guarded_log_line(rsf_game_log_fn log, void* user, const char* message)
{
    (void)rsf_game_log_guarded(log, user, message);
}
