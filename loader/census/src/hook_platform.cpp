// SPDX-License-Identifier: MIT
// Native MinHook and guarded Windows loader-list access for the census binding owner.
#include "MinHook.h"
#include "census_internal.h"

namespace rsf_census {
namespace {

void register_wrappers(void** table) noexcept {
    register_wrappers_d3d(table);
    register_wrappers_vk(table);
    register_wrappers_ngx(table);
    register_wrappers_fsr(table);
    register_wrappers_misc(table);
}

} // namespace

bool hooks_install() noexcept {
    static_assert(kFnCount < 1024);
    static const rsf_census_hook_platform platform = {
        kFns, static_cast<uint16_t>(kFnCount),
        {static_cast<uint16_t>(Fn::CreateDXGIFactory), static_cast<uint16_t>(Fn::CreateDXGIFactory1),
         static_cast<uint16_t>(Fn::CreateDXGIFactory2), static_cast<uint16_t>(Fn::D3D12CreateDevice)},
        []() -> int { return MH_Initialize(); }, register_wrappers,
        [](const uint16_t* module, const char* name, void* detour, void** original) -> int {
            return MH_CreateHookApi(reinterpret_cast<const wchar_t*>(module), name, detour, original);
        },
        [](void* target, void* detour, void** original) -> int {
            return MH_CreateHook(target, detour, original);
        },
        [](void* target) { MH_RemoveHook(target); },
        [](void* target) -> int { return MH_QueueEnableHook(target); },
        []() { MH_ApplyQueued(); },
        []() { MH_DisableHook(MH_ALL_HOOKS); },
        []() -> const void* { return NtCurrentTeb(); }, safe_read, read_arg_string, installed,
    };
    return rsf_census_hooks_install(&platform, option_flags());
}

} // namespace rsf_census
