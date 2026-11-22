// SPDX-License-Identifier: MIT
// The SEH bodies the C leaves share: a guarded engine-memory copy and a call into a foreign log sink.
#pragma once
#include <windows.h>
#include <stdbool.h>
#include <string.h>

static inline bool guarded_copy(void* output, const void* input, size_t size)
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

// The log sink is foreign code. Under MSVC a C++ exception it raises (0xE06D7363) is contained here, as narrow as the
// former catch (...) under /EHsc, and its object abandoned; any other exception continues the search. GCC C has no
// handler for a foreign C++ exception. False when an exception was contained.
static inline bool guarded_log(void (*log)(void*, const char*), void* user, const char* message)
{
    if (!log) return true;
#ifdef _MSC_VER
    __try {
        log(user, message);
    } __except (GetExceptionCode() == 0xE06D7363u ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) {
        return false;
    }
#else
    log(user, message);
#endif
    return true;
}

static inline void guarded_log_line(void (*log)(void*, const char*), void* user, const char* message)
{
    (void)guarded_log(log, user, message);
}
