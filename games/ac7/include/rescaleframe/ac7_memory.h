/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* Reads of live engine memory that must fail soft. Internal to the AC7 module. */
#include <windows.h>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace rsf::ac7 {
/* No C++ objects in the SEH scope. These are live engine arguments, but a mismatched layout must
   produce an unreadable record rather than a second fault inside diagnostic code. Without MSVC
   there is no __try, so the range is checked with VirtualQuery instead (not race-free, but the
   readers only ever see committed engine pages). */
inline bool copy_memory(void* output, const void* input, size_t size)
{
    if (!output || !input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION m{};
    if (!VirtualQuery(input, &m, sizeof(m)) || m.State != MEM_COMMIT ||
        (m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) ||
        uintptr_t(input) + size > uintptr_t(m.BaseAddress) + m.RegionSize) return false;
    std::memcpy(output, input, size); return true;
#endif
}
/* T at base + offset. A null base reads nothing. */
template<class T> bool read(const void* base, size_t offset, T& output)
{
    return base && copy_memory(&output, static_cast<const unsigned char*>(base) + offset, sizeof(T));
}
template<class T> bool read(uint64_t base, size_t offset, T& output)
{
    return base && copy_memory(&output, reinterpret_cast<const void*>(uintptr_t(base) + offset), sizeof(T));
}
}
