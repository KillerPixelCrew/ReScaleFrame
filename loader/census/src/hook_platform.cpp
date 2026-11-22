// SPDX-License-Identifier: MIT
// Native MinHook and TEB entry points for the Rust binding owner.
#include "MinHook.h"
#include "census_internal.h"

namespace rsf_census {
static_assert(kFnCount < 1024);

namespace {

int initialize() noexcept { return MH_Initialize(); }

int create_api(const uint16_t* module, const char* name, void* detour, void** original) noexcept {
    return MH_CreateHookApi(reinterpret_cast<const wchar_t*>(module), name, detour, original);
}

int create(void* target, void* detour, void** original) noexcept {
    return MH_CreateHook(target, detour, original);
}

void remove(void* target) noexcept { MH_RemoveHook(target); }
int enable(void* target) noexcept { return MH_QueueEnableHook(target); }
void apply() noexcept { MH_ApplyQueued(); }
void disable() noexcept { MH_DisableHook(MH_ALL_HOOKS); }
const void* current_teb() noexcept { return NtCurrentTeb(); }

} // namespace

constinit const rsf_census_hook_platform kHookPlatform = {
    kFns, static_cast<uint16_t>(kFnCount),
    {static_cast<uint16_t>(Fn::CreateDXGIFactory), static_cast<uint16_t>(Fn::CreateDXGIFactory1),
     static_cast<uint16_t>(Fn::CreateDXGIFactory2), static_cast<uint16_t>(Fn::D3D12CreateDevice)},
    initialize, register_wrappers_d3d, create_api, create, remove, enable, apply, disable,
    current_teb, safe_read, read_arg_string,
};

} // namespace rsf_census
