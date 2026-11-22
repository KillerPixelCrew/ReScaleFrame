/* SPDX-License-Identifier: MIT */
/* Native MinHook and TEB entry points for the Rust binding owner. */
#include "MinHook.h"
#include "census_internal.h"

_Static_assert(RSF_FN_COUNT < 1024, "the hook table indexes functions with 10 bits");

static int hook_initialize(void) { return MH_Initialize(); }

static int hook_create_api(const uint16_t* module, const char* name, void* detour, void** original)
{
    return MH_CreateHookApi((const wchar_t*)module, name, detour, original);
}

static int hook_create(void* target, void* detour, void** original)
{
    return MH_CreateHook(target, detour, original);
}

static void hook_remove(void* target) { MH_RemoveHook(target); }
static int hook_enable(void* target) { return MH_QueueEnableHook(target); }
static void hook_apply(void) { MH_ApplyQueued(); }
static void hook_disable(void) { MH_DisableHook(MH_ALL_HOOKS); }
static const void* hook_current_teb(void) { return NtCurrentTeb(); }

const rsf_census_hook_platform rsf_census_hook_platform_table = {
    rsf_census_functions,
    (uint16_t)RSF_FN_COUNT,
    {(uint16_t)RSF_FN_CreateDXGIFactory, (uint16_t)RSF_FN_CreateDXGIFactory1,
     (uint16_t)RSF_FN_CreateDXGIFactory2, (uint16_t)RSF_FN_D3D12CreateDevice},
    hook_initialize,
    rsf_census_register_wrappers_d3d,
    hook_create_api,
    hook_create,
    hook_remove,
    hook_enable,
    hook_apply,
    hook_disable,
    hook_current_teb,
    rsf_census_safe_read,
    rsf_census_read_arg_string,
};
