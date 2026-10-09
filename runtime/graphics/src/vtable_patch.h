/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* Internal to rsf_graphics: replace one COM vtable slot, remembering what was there. */
#include <windows.h>
#include <cstddef>

namespace rsf {
inline bool patch_slot(void** vtable, size_t index, void* replacement, void** previous)
{
    DWORD protection = 0;
    if (!VirtualProtect(&vtable[index], sizeof(void*), PAGE_EXECUTE_READWRITE, &protection)) {
        return false;
    }
    if (previous) {
        *previous = vtable[index];
    }
    vtable[index] = replacement;
    DWORD restored = 0;
    VirtualProtect(&vtable[index], sizeof(void*), protection, &restored);
    return true;
}
}
