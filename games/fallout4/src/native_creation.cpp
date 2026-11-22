// SPDX-License-Identifier: MIT
// Native leaves of the Rust creation owner (creation.rs). The detour on the creation import thunk
// passes the engine's own arguments and its return address to Rust; Rust decides the scope and
// calls the unchanged native creation through call_creation.
#include "native_creation.h"
#include <d3d11.h>
#include <windows.h>
#include <cstddef>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::fallout4 {
namespace {
static_assert(sizeof(rsf_fallout4_create_call) == 88 && offsetof(rsf_fallout4_create_call, immediate_context) == 80);
static_assert(sizeof(D3D_DRIVER_TYPE) == sizeof(int32_t) && sizeof(D3D_FEATURE_LEVEL) == sizeof(int32_t));
using NativeCreate = decltype(&D3D11CreateDeviceAndSwapChain);
HRESULT WINAPI observe_create(IDXGIAdapter* adapter, D3D_DRIVER_TYPE type, HMODULE software, UINT flags,
    const D3D_FEATURE_LEVEL* levels, UINT level_count, UINT sdk, const DXGI_SWAP_CHAIN_DESC* desc,
    IDXGISwapChain** chain, ID3D11Device** device, D3D_FEATURE_LEVEL* selected, ID3D11DeviceContext** context) {
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const rsf_fallout4_create_call call{adapter, int32_t(type), software, flags,
        reinterpret_cast<const int32_t*>(levels), level_count, sdk, desc,
        reinterpret_cast<void**>(chain), reinterpret_cast<void**>(device),
        reinterpret_cast<int32_t*>(selected), reinterpret_cast<void**>(context)};
    return rsf_fallout4_creation_observe(caller, &call);
}
}
extern "C" void* rsf_fallout4_creation_detour() noexcept {
    return reinterpret_cast<void*>(&observe_create);
}
int32_t call_creation(const rsf_fallout4_create_call* call) noexcept {
    const auto native_create = reinterpret_cast<NativeCreate>(rsf_fallout4_creation_original());
    if (!native_create) return E_FAIL;
    return native_create(static_cast<IDXGIAdapter*>(call->adapter), D3D_DRIVER_TYPE(call->driver_type),
        static_cast<HMODULE>(call->software), call->flags,
        reinterpret_cast<const D3D_FEATURE_LEVEL*>(call->feature_levels), call->feature_level_count,
        call->sdk_version, static_cast<const DXGI_SWAP_CHAIN_DESC*>(call->swapchain_desc),
        reinterpret_cast<IDXGISwapChain**>(call->swapchain), reinterpret_cast<ID3D11Device**>(call->device),
        reinterpret_cast<D3D_FEATURE_LEVEL*>(call->feature_level),
        reinterpret_cast<ID3D11DeviceContext**>(call->immediate_context));
}
}
