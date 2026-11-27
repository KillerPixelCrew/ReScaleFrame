/* SPDX-License-Identifier: MIT */
#include <rescaleframe/foreign_guard.h>
#include <rescaleframe/ac7_memory.h>
#include <string.h>
#include <windows.h>

bool rsf_ac7_copy_memory(void* output, const void* input, size_t size)
{
    if (!output || !input) return false;
#if defined(_MSC_VER)
    RSF_FOREIGN_TRY {
        memcpy(output, input, size);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
#else
    /* This check cannot protect against concurrent page changes. */
    MEMORY_BASIC_INFORMATION region = {0};
    if (!VirtualQuery(input, &region, sizeof(region)) || region.State != MEM_COMMIT ||
        (region.Protect & (PAGE_NOACCESS | PAGE_GUARD))) return false;
    const uintptr_t offset = (uintptr_t)input - (uintptr_t)region.BaseAddress;
    if (offset > region.RegionSize || size > region.RegionSize - offset) return false;
    memcpy(output, input, size);
    return true;
#endif
}

bool rsf_ac7_read_memory(void* output, uintptr_t base, size_t offset, size_t size)
{
    return base && offset <= UINTPTR_MAX - base &&
        rsf_ac7_copy_memory(output, (const void*)(base + offset), size);
}
