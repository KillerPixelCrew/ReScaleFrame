// SPDX-License-Identifier: MIT
#include <windows.h>
#include <cstdint>
#include <cstring>

// Copy only at the native fault boundary. The CPU and creation owners read into
// their own storage. MSVC contains access violations in the copy itself.
extern "C" bool rsf_skyrim_guarded_read(uintptr_t address, void* output, size_t size) noexcept {
    if (!output) return false;
    const auto pointer = reinterpret_cast<const void*>(address);
#if defined(_MSC_VER)
    __try { std::memcpy(output, pointer, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    if (!VirtualQuery(pointer, &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD))) return false;
    const auto offset = address - uintptr_t(memory.BaseAddress);
    if (offset > memory.RegionSize || size > memory.RegionSize - offset) return false;
    std::memcpy(output, pointer, size);
    return true;
#endif
}
