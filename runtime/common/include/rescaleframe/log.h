/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* The printf-style log helper every library had its own copy of. The sinks are all
   `void (*)(void* user, const char* message)`, so one function covers them whatever the typedef is
   called. Messages longer than 512 bytes are truncated by vsnprintf, as in the copies. */
#include <cstdarg>
#include <cstdio>

#if defined(__GNUC__) || defined(__clang__)
#define RSF_PRINTF_LIKE(fmt_index, first_arg) __attribute__((format(printf, fmt_index, first_arg)))
#else
#define RSF_PRINTF_LIKE(format, first)
#endif

namespace rsf {
RSF_PRINTF_LIKE(3, 4)
inline void say(void (*log)(void* user, const char* message), void* user, const char* format, ...)
{
    if (!log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    log(user, message);
}
}
