/* SPDX-License-Identifier: MIT */
#pragma once
/* MinHook batch effects for the AC7 capture. Internal. */
#include <MinHook.h>
#include <cstdint>

namespace rsf::ac7 {
/* Clear retired targets. Keep failed removals for a later retry. */
inline void remove_hooks(void** targets, uint32_t count)
{
    for (uint32_t i = 0; i < count; ++i) {
        if (!targets[i]) continue;
        MH_DisableHook(targets[i]);
        const auto result = MH_RemoveHook(targets[i]);
        if (result == MH_OK || result == MH_ERROR_NOT_CREATED) targets[i] = nullptr;
    }
}
/* Create and enable `count` hooks. A null target is a site deliberately left alone (an entry the
   caller skips) and is neither created nor counted as a failure. On failure, only hooks that
   this call created and could not remove stay in the target array. */
inline bool install_hooks(void** targets, void* const* detours, void** const* originals, uint32_t count)
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
        for (uint32_t i = made; i < count; ++i) targets[i] = nullptr;
        remove_hooks(targets, made);
    }
    return ok;
}
}
