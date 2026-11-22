/* SPDX-License-Identifier: MIT */
#pragma once
/* MinHook batch effects for the AC7 capture. Internal. */
#include <MinHook.h>
#include <cstdint>

extern "C" {
typedef bool (*rsf_ac7_hook_create_fn)(void*, void*, void**);
typedef bool (*rsf_ac7_hook_fn)(void*);
bool rsf_ac7_install_hook_batch(void**, void* const*, void** const*, uint32_t,
    rsf_ac7_hook_create_fn, rsf_ac7_hook_fn, rsf_ac7_hook_fn, rsf_ac7_hook_fn);
void rsf_ac7_remove_hook_batch(void**, uint32_t, rsf_ac7_hook_fn, rsf_ac7_hook_fn);
}

namespace rsf::ac7 {
inline bool disable_hook(void* target)
{
    return MH_DisableHook(target) == MH_OK;
}
inline bool retire_hook(void* target)
{
    const auto result = MH_RemoveHook(target);
    return result == MH_OK || result == MH_ERROR_NOT_CREATED;
}
inline void remove_hooks(void** targets, uint32_t count)
{
    rsf_ac7_remove_hook_batch(targets, count, disable_hook, retire_hook);
}
inline bool install_hooks(void** targets, void* const* detours, void** const* originals, uint32_t count)
{
    return rsf_ac7_install_hook_batch(targets, detours, originals, count,
        [](void* target, void* detour, void** original) {
            return MH_CreateHook(target, detour, original) == MH_OK;
        },
        [](void* target) { return MH_EnableHook(target) == MH_OK; },
        disable_hook, retire_hook);
}
}
