/* SPDX-License-Identifier: MIT */
#pragma once
#include <cstddef>
#include <cstdint>

extern "C" int32_t rsf_graphics_replace_vtable_slot(void** address, void* expected, void* replacement);

namespace rsf {
inline bool patch_slot(void** vtable, size_t index, void* replacement, void** previous)
{
    auto** address = vtable + index;
    void* original = *reinterpret_cast<void* volatile*>(address);
    if (!rsf_graphics_replace_vtable_slot(address, original, replacement)) {
        return false;
    }
    if (previous) {
        *previous = original;
    }
    return true;
}
}
