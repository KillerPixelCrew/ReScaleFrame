/* SPDX-License-Identifier: MIT */
/* Native D3D11 call frame and foreign Runtime exception guards for creation.rs. */
#include "native_creation_platform.h"
#include <windows.h>
#include <d3d11.h>
#include <stddef.h>
#include <string.h>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

typedef int32_t (*creation_begin_fn)(void* user, rsf_d3d11_creation_scope* scope);
typedef int32_t (*creation_end_fn)(void* user, const rsf_d3d11_creation_scope* scope,
    const rsf_d3d11_native_chain* outputs);

/* creation.rs reads OutputWindow from the borrowed description at this offset. */
_Static_assert(sizeof(DXGI_SWAP_CHAIN_DESC) == 72 && offsetof(DXGI_SWAP_CHAIN_DESC, OutputWindow) == 48,
    "DXGI swap chain description layout");
_Static_assert(sizeof(rsf_d3d11_creation_scope) == 24 && sizeof(rsf_d3d11_native_chain) == 32,
    "D3D11 creation record layout");
_Static_assert(sizeof(rsf_skyrim_creation_call) == 136 && offsetof(rsf_skyrim_creation_call, native_result) == 48 &&
    offsetof(rsf_skyrim_creation_call, owner) == 56 && offsetof(rsf_skyrim_creation_call, scope) == 80 &&
    offsetof(rsf_skyrim_creation_call, outputs) == 104, "Skyrim creation call layout");

/* Leave closes an admitted Runtime scope after the original returns or unwinds, then restores the
   native last error. The original keeps the current IAT and every original argument. */
HRESULT WINAPI rsf_skyrim_create_detour(IDXGIAdapter* adapter, D3D_DRIVER_TYPE driver, HMODULE software, UINT flags,
    const D3D_FEATURE_LEVEL* levels, UINT level_count, UINT sdk, const DXGI_SWAP_CHAIN_DESC* description,
    IDXGISwapChain** chain, ID3D11Device** device, D3D_FEATURE_LEVEL* selected, ID3D11DeviceContext** context)
{
    rsf_skyrim_creation_call call;
    HRESULT result = E_FAIL;
    memset(&call, 0, sizeof(call));
    call.struct_size = sizeof(call);
    call.version = RSF_SKYRIM_CREATION_PLATFORM_VERSION;
#if defined(_MSC_VER)
    call.caller = (uintptr_t)_ReturnAddress();
#else
    call.caller = (uintptr_t)__builtin_return_address(0);
#endif
    call.description = description;
    call.swapchain = (void**)chain;
    call.device = (void**)device;
    call.context = (void**)context;
    call.native_result = E_FAIL;
#if defined(_MSC_VER)
    __try {
#endif
        const PFN_D3D11_CREATE_DEVICE_AND_SWAP_CHAIN original =
            (PFN_D3D11_CREATE_DEVICE_AND_SWAP_CHAIN)rsf_skyrim_creation_enter(&call);
        /* A null original is unreachable: MinHook enables this detour only after the crate
           published the trampoline. */
        if (original) {
            call.native_result = original(adapter, driver, software, flags, levels, level_count, sdk,
                description, chain, device, selected, context);
            call.returned = 1;
            /* End borrows outputs; the outer native HRESULT is unchanged. */
            result = call.native_result;
        }
#if defined(_MSC_VER)
    } __finally {
        rsf_skyrim_creation_leave(&call);
    }
#else
    rsf_skyrim_creation_leave(&call);
#endif
    return result;
}

/* The Runtime callbacks cross a C ABI, but their implementation may raise a foreign exception. */
int32_t rsf_skyrim_creation_begin_guarded(uintptr_t address, void* user, rsf_d3d11_creation_scope* scope)
{
#if defined(_MSC_VER)
    __try {
        return ((creation_begin_fn)address)(user, scope);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return RSF_ERROR_NOT_READY;
    }
#else
    return ((creation_begin_fn)address)(user, scope);
#endif
}

int32_t rsf_skyrim_creation_end_guarded(uintptr_t address, void* user, const rsf_d3d11_creation_scope* scope,
    const rsf_d3d11_native_chain* outputs)
{
#if defined(_MSC_VER)
    __try {
        return ((creation_end_fn)address)(user, scope, outputs);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return RSF_ERROR_NOT_READY;
    }
#else
    return ((creation_end_fn)address)(user, scope, outputs);
#endif
}