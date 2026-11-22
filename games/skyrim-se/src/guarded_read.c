// SPDX-License-Identifier: MIT
#include <windows.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

// Copy only at the native fault boundary. The CPU and creation owners read into
// their own storage. MSVC contains access violations in the copy itself.
bool rsf_skyrim_guarded_read(uintptr_t address, void* output, size_t size)
{
    if (!output) return false;
    const void* pointer = (const void*)address;
#if defined(_MSC_VER)
    __try { memcpy(output, pointer, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory = {0};
    if (!VirtualQuery(pointer, &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD))) return false;
    const uintptr_t offset = address - (uintptr_t)memory.BaseAddress;
    if (offset > memory.RegionSize || size > memory.RegionSize - offset) return false;
    memcpy(output, pointer, size);
    return true;
#endif
}
