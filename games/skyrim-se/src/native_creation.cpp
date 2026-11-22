// SPDX-License-Identifier: MIT
// Native leaves of the Skyrim SE native-creation owner. The Rust crate rsf-skyrim-se
// (src/creation.rs, src/layout.rs) owns the guarded thunk and route profiles, the cold-owner
// rule, admission order, Runtime scope transaction, refusals and messages. This file keeps the
// fault-guarded read, MinHook, exception guards around host
// and Runtime callbacks, the Community Shaders gate, and the detour that
// captures the native return address and calls the original on its own frame.
#include "native_creation_platform.h"
#include "observer_platform.h"
#include "compatibility.h"
#include <MinHook.h>
#include <windows.h>
#include <d3d11.h>
#include <cstddef>
#include <cstring>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::skyrim {
namespace {
using Create = decltype(&D3D11CreateDeviceAndSwapChain);
using Begin = decltype(&rsf_d3d11_host_creation_begin);
using End = decltype(&rsf_d3d11_host_creation_end);
// creation.rs reads OutputWindow from the borrowed description at this offset.
static_assert(sizeof(DXGI_SWAP_CHAIN_DESC) == 72 && offsetof(DXGI_SWAP_CHAIN_DESC, OutputWindow) == 48);
static_assert(sizeof(rsf_d3d11_creation_scope) == 24 && sizeof(rsf_d3d11_native_chain) == 32);
static_assert(sizeof(rsf_skyrim_creation_call) == 136 && offsetof(rsf_skyrim_creation_call, native_result) == 48 &&
    offsetof(rsf_skyrim_creation_call, owner) == 56 && offsetof(rsf_skyrim_creation_call, scope) == 80 &&
    offsetof(rsf_skyrim_creation_call, outputs) == 104);
static_assert(sizeof(rsf_skyrim_creation_platform) == 16 + 9 * sizeof(void*) &&
    offsetof(rsf_skyrim_creation_platform, create_detour) == 16 + 4 * sizeof(void*));

bool read(uintptr_t address, void* destination, size_t size) noexcept {
#if defined(_MSC_VER)
    __try { std::memcpy(destination, reinterpret_cast<const void*>(address), size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    if (!VirtualQuery(reinterpret_cast<void*>(address), &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) || address + size < address ||
        address + size > uintptr_t(memory.BaseAddress) + memory.RegionSize) return false;
    std::memcpy(destination, reinterpret_cast<const void*>(address), size); return true;
#endif
}

HRESULT WINAPI create(IDXGIAdapter* adapter, D3D_DRIVER_TYPE driver, HMODULE software, UINT flags,
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

const rsf_skyrim_creation_platform platform{
    sizeof(rsf_skyrim_creation_platform), RSF_SKYRIM_CREATION_PLATFORM_VERSION, RSF_D3D11_HOST_ABI_VERSION, 0,
    [](uintptr_t address, void* out, size_t size) noexcept { return out && read(address, out, size); },
    [](uintptr_t target, const void* detour, void** original) noexcept {
        return MH_CreateHook(reinterpret_cast<void*>(target), const_cast<void*>(detour), original) == MH_OK;
    },
    [](uintptr_t target) noexcept {
        const auto status = MH_EnableHook(reinterpret_cast<void*>(target));
        return status == MH_OK || status == MH_ERROR_ENABLED;
    },
    [](uintptr_t target) noexcept {
        const auto status = MH_DisableHook(reinterpret_cast<void*>(target));
        return status == MH_OK || status == MH_ERROR_DISABLED;
    },
    reinterpret_cast<const void*>(&create),
    [](const rsf_game_host_services* host, const char* message) noexcept {
        if (host && host->log) try { host->log(host->user, message); } catch (...) {}
    },
    [](uintptr_t service, void* user, rsf_d3d11_creation_scope* scope) noexcept {
        int32_t status = RSF_ERROR_NOT_READY;
        try { status = reinterpret_cast<Begin>(service)(user, scope); } catch (...) {}
        return status;
    },
    [](uintptr_t service, void* user, const rsf_d3d11_creation_scope* scope,
        const rsf_d3d11_native_chain* outputs) noexcept {
        int32_t status = RSF_ERROR_NOT_READY;
        try { status = reinterpret_cast<End>(service)(user, scope, outputs); } catch (...) {}
        return status;
    },
    []() noexcept { return compatibility_render_allowed(); },
};
}

const rsf_skyrim_creation_platform* creation_platform() noexcept { return &platform; }
}
