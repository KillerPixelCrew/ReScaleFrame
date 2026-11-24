/* SPDX-License-Identifier: MIT */
/* Native exception guards and the typed creation call used by the Rust adapter (lib.rs and
   creation.rs). Rust must never see a foreign C++ exception, so each Runtime call that can throw
   runs behind the native filter in native_guard.h and reports false instead. */
#include "fallout4_platform.h"
#include "native_guard.h"
#include <d3d11.h>
#include <stddef.h>

/* A native call that throws reports false; the Rust lifecycle then answers RSF_ERROR_NOT_READY. */

bool rsf_fallout4_native_host_create(rsf_fallout4_host_create_fn create, const rsf_d3d11_host_setup* setup,
    rsf_game_host_services* services, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = create(setup, services); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_native_host_set_active(rsf_fallout4_host_active_fn activate, void* user, uint32_t active,
    int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = activate(user, active); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_native_creation_begin(rsf_fallout4_creation_begin_fn begin, void* user,
    rsf_d3d11_creation_scope* scope, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = begin(user, scope); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_native_creation_end(rsf_fallout4_creation_end_fn end, void* user,
    const rsf_d3d11_creation_scope* scope, const rsf_d3d11_native_chain* outputs, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = end(user, scope, outputs); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_native_bind_presentation(rsf_fallout4_bind_presentation_fn bind, void* user,
    const rsf_d3d11_presentation_binding* binding, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = bind(user, binding); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

/* The native layout of the call record and the D3D enumerations the Rust side passes as 32-bit values. */
_Static_assert(sizeof(rsf_fallout4_create_call) == 88 && offsetof(rsf_fallout4_create_call, immediate_context) == 80,
    "rsf_fallout4_create_call layout");
_Static_assert(sizeof(D3D_DRIVER_TYPE) == sizeof(int32_t) && sizeof(D3D_FEATURE_LEVEL) == sizeof(int32_t),
    "D3D enumerations are 32-bit");

/* An engine throw ends the process: nothing catches it here, so the creation call never resumes
   a half-created device chain. */
int32_t rsf_fallout4_call_creation(void* original, const rsf_fallout4_create_call* call)
{
    const PFN_D3D11_CREATE_DEVICE_AND_SWAP_CHAIN native_create =
        (PFN_D3D11_CREATE_DEVICE_AND_SWAP_CHAIN)original;
    return (int32_t)native_create((IDXGIAdapter*)call->adapter, (D3D_DRIVER_TYPE)call->driver_type,
        (HMODULE)call->software, call->flags, (const D3D_FEATURE_LEVEL*)call->feature_levels,
        call->feature_level_count, call->sdk_version, (const DXGI_SWAP_CHAIN_DESC*)call->swapchain_desc,
        (IDXGISwapChain**)call->swapchain, (ID3D11Device**)call->device,
        (D3D_FEATURE_LEVEL*)call->feature_level, (ID3D11DeviceContext**)call->immediate_context);
}
