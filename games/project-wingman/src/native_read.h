// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace rsf::wingman {
inline bool copy(void* output, const void* input, size_t bytes) noexcept
{
    if (!output || !input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, bytes); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION page{};
    if (!VirtualQuery(input, &page, sizeof(page)) || page.State != MEM_COMMIT ||
        (page.Protect & (PAGE_GUARD | PAGE_NOACCESS))) return false;
    const auto address = reinterpret_cast<uintptr_t>(input);
    const auto base = reinterpret_cast<uintptr_t>(page.BaseAddress);
    if (address < base || bytes > page.RegionSize || address - base > page.RegionSize - bytes) return false;
    std::memcpy(output, input, bytes);
    return true;
#endif
}

template<class T> bool read(const void* base, size_t offset, T& value) noexcept
{
    const auto address = reinterpret_cast<uintptr_t>(base);
    return address && offset <= UINTPTR_MAX - address &&
        copy(&value, reinterpret_cast<const void*>(address + offset), sizeof(value));
}
}
