/* SPDX-License-Identifier: MIT */
#include <rescaleframe/ac7_hooks.h>
#include <MinHook.h>

static bool create(void* target, void* detour, void** original)
{
    return MH_CreateHook(target, detour, original) == MH_OK;
}

static bool enable(void* target)
{
    return MH_EnableHook(target) == MH_OK;
}

static bool disable(void* target)
{
    return MH_DisableHook(target) == MH_OK;
}

static bool retire(void* target)
{
    const MH_STATUS result = MH_RemoveHook(target);
    return result == MH_OK || result == MH_ERROR_NOT_CREATED;
}

bool rsf_ac7_capture_initialize_minhook(void)
{
    const MH_STATUS result = MH_Initialize();
    return result == MH_OK || result == MH_ERROR_ALREADY_INITIALIZED;
}

bool rsf_ac7_capture_install_hooks(void** targets, void* const* detours, void** const* originals, uint32_t count)
{
    return rsf_ac7_install_hook_batch(targets, detours, originals, count, create, enable, disable, retire);
}

void rsf_ac7_capture_remove_hook_batch(void** targets, uint32_t count)
{
    rsf_ac7_remove_hook_batch(targets, count, disable, retire);
}
