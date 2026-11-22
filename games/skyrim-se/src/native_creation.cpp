// SPDX-License-Identifier: MIT
// Native D3D11 call frame and foreign Runtime exception guards for creation.rs.
#include "native_creation_platform.h"
#include <windows.h>
#include <d3d11.h>
#include <cstddef>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

using Create = decltype(&D3D11CreateDeviceAndSwapChain);
using Begin = decltype(&rsf_d3d11_host_creation_begin);
using End = decltype(&rsf_d3d11_host_creation_end);
// creation.rs reads OutputWindow from the borrowed description at this offset.
static_assert(sizeof(DXGI_SWAP_CHAIN_DESC) == 72 && offsetof(DXGI_SWAP_CHAIN_DESC, OutputWindow) == 48);
static_assert(sizeof(rsf_d3d11_creation_scope) == 24 && sizeof(rsf_d3d11_native_chain) == 32);
static_assert(sizeof(rsf_skyrim_creation_call) == 136 && offsetof(rsf_skyrim_creation_call, native_result) == 48 &&
    offsetof(rsf_skyrim_creation_call, owner) == 56 && offsetof(rsf_skyrim_creation_call, scope) == 80 &&
    offsetof(rsf_skyrim_creation_call, outputs) == 104);
extern "C" HRESULT WINAPI rsf_skyrim_create_detour(IDXGIAdapter* adapter, D3D_DRIVER_TYPE driver, HMODULE software, UINT flags,
    const D3D_FEATURE_LEVEL* levels, UINT level_count, UINT sdk, const DXGI_SWAP_CHAIN_DESC* description,
    IDXGISwapChain** chain, ID3D11Device** device, D3D_FEATURE_LEVEL* selected, ID3D11DeviceContext** context) {
    rsf_skyrim_creation_call call{};
    call.struct_size = sizeof(call);
    call.version = RSF_SKYRIM_CREATION_PLATFORM_VERSION;
#if defined(_MSC_VER)
    call.caller = uintptr_t(_ReturnAddress());
#else
    call.caller = uintptr_t(__builtin_return_address(0));
#endif
    call.description = description;
    call.swapchain = reinterpret_cast<void**>(chain);
    call.device = reinterpret_cast<void**>(device);
    call.context = reinterpret_cast<void**>(context);
    call.native_result = E_FAIL;
    // Leave closes an admitted Runtime scope after the original returns or unwinds, then restores
    // the native last error. The original keeps the current IAT and every original argument.
    struct Leave {
        rsf_skyrim_creation_call& call;
        ~Leave() { rsf_skyrim_creation_leave(&call); }
    } leave{call};
    const auto original = reinterpret_cast<Create>(rsf_skyrim_creation_enter(&call));
    // Unreachable: MinHook enables this detour only after the crate published the trampoline.
    if (!original) return E_FAIL;
    call.native_result = original(adapter, driver, software, flags, levels, level_count, sdk,
        description, chain, device, selected, context);
    call.returned = 1;
    return call.native_result; // End borrows outputs; the outer native HRESULT is unchanged.
}

extern "C" int32_t rsf_skyrim_creation_begin_guarded(
    uintptr_t address, void* user, rsf_d3d11_creation_scope* scope) noexcept try
{
    return reinterpret_cast<Begin>(address)(user, scope);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" int32_t rsf_skyrim_creation_end_guarded(
    uintptr_t address, void* user, const rsf_d3d11_creation_scope* scope,
    const rsf_d3d11_native_chain* outputs) noexcept try
{
    return reinterpret_cast<End>(address)(user, scope, outputs);
} catch (...) { return RSF_ERROR_NOT_READY; }
