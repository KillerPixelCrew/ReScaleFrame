/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* All-or-nothing MinHook installation for the AC7 module. Internal. */
#include <MinHook.h>
#include <cstdint>

namespace rsf::ac7 {
/* Remove hooks that were created: disable, then MH_RemoveHook. Null targets are skipped. */
inline void remove_hooks(void* const* targets, uint32_t count)
{
    for (uint32_t i = 0; i < count; ++i) {
        if (!targets[i]) continue;
        MH_DisableHook(targets[i]);
        MH_RemoveHook(targets[i]);
    }
}
/* Create and enable `count` hooks. A null target is a site deliberately left alone (an entry the
   caller skips) and is neither created nor counted as a failure. Any failure rolls back every
   hook this call made, disabled and removed, and returns false with the originals untouched. */
inline bool install_hooks(void* const* targets, void* const* detours, void** const* originals, uint32_t count)
{
    uint32_t made = 0;
    bool ok = true;
    for (; made < count; ++made) {
        if (!targets[made]) continue;
        if (MH_CreateHook(targets[made], detours[made], originals[made]) != MH_OK) { ok = false; break; }
    }
    for (uint32_t i = 0; ok && i < made; ++i) {
        if (targets[i] && MH_EnableHook(targets[i]) != MH_OK) ok = false;
    }
    if (!ok) {
        // A failed create at index `made` made nothing, so only [0, made) can need removing.
        remove_hooks(targets, made);
    }
    return ok;
}
}
