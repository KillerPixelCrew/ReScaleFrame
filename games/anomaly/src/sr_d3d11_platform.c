/* SPDX-License-Identifier: MIT */
/* Native leaves of the SR D3D11 table. Rust owns the table and every typed COM call; this file keeps the shared
   default-view and pipeline-state helpers, the C++ exception boundary around foreign log sinks and the descriptor
   layout checks against the Windows SDK. */
#include "sr_d3d11_platform.h"
#include "sr_swap_platform.h"
#include <rescaleframe/d3d11_state.h>
#include <windows.h>
#include <d3d11.h>
#include <stddef.h>
#include <stdlib.h>

_Static_assert(sizeof(D3D11_TEXTURE2D_DESC) == RSF_ANOMALY_SWAP_DESC_SIZE &&
    sizeof(rsf_sr_texture) == RSF_ANOMALY_SWAP_DESC_SIZE, "D3D11_TEXTURE2D_DESC size");
_Static_assert(offsetof(D3D11_TEXTURE2D_DESC, MipLevels) == offsetof(rsf_sr_texture, mips) &&
    offsetof(D3D11_TEXTURE2D_DESC, ArraySize) == offsetof(rsf_sr_texture, layers) &&
    offsetof(D3D11_TEXTURE2D_DESC, Format) == offsetof(rsf_sr_texture, format) &&
    offsetof(D3D11_TEXTURE2D_DESC, SampleDesc) == offsetof(rsf_sr_texture, samples) &&
    offsetof(D3D11_TEXTURE2D_DESC, Usage) == offsetof(rsf_sr_texture, usage) &&
    offsetof(D3D11_TEXTURE2D_DESC, BindFlags) == offsetof(rsf_sr_texture, bindings) &&
    offsetof(D3D11_TEXTURE2D_DESC, CPUAccessFlags) == offsetof(rsf_sr_texture, cpu_access) &&
    offsetof(D3D11_TEXTURE2D_DESC, MiscFlags) == offsetof(rsf_sr_texture, misc), "D3D11_TEXTURE2D_DESC fields");
_Static_assert(DXGI_FORMAT_UNKNOWN == 0 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R32_FLOAT == 41 &&
    DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_D24_UNORM_S8_UINT == 45, "DXGI formats used by the twins");
_Static_assert(DXGI_FORMAT_R8G8B8A8_TYPELESS == 27 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_FORMAT_B8G8R8A8_UNORM == 87 && DXGI_FORMAT_B8G8R8A8_TYPELESS == 90, "DXGI UNORM8 formats");
_Static_assert(D3D11_USAGE_DEFAULT == 0 && D3D11_USAGE_IMMUTABLE == 1, "D3D11 usages");
_Static_assert(D3D11_BIND_SHADER_RESOURCE == 0x08 && D3D11_BIND_RENDER_TARGET == 0x20 &&
    D3D11_BIND_DEPTH_STENCIL == 0x40 && D3D11_BIND_UNORDERED_ACCESS == 0x80, "D3D11 bind flags");
_Static_assert((D3D11_FORMAT_SUPPORT_TEXTURE2D | D3D11_FORMAT_SUPPORT_SHADER_LOAD | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE |
    D3D11_FORMAT_SUPPORT_RENDER_TARGET | D3D11_FORMAT_SUPPORT_BLENDABLE) == 0xc320 &&
    D3D11_FORMAT_SUPPORT_TYPED_UNORDERED_ACCESS_VIEW == 0x02000000, "D3D11 format support bits");

static void* default_view(void* device, void* texture)
{
    return rsf_d3d11_create_shader_view(device, texture);
}

#ifdef _MSC_VER
/* The MSVC C++ exception code. Filtering on it keeps the boundary as narrow as catch (...) under /EHsc: hardware
   faults still reach the process handlers. */
static int cpp_exception(DWORD code)
{
    return code == 0xe06d7363u ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH;
}
#endif

static bool guarded_log(rsf_sr_log log, void* user, const char* message)
{
    if (!log) return true;
#ifdef _MSC_VER
    __try {
        log(user, message);
    } __except (cpp_exception(GetExceptionCode())) {
        return false;
    }
#else
    log(user, message);
#endif
    return true;
}

static void* save_state(void* context)
{
    rsf_d3d11_state* state = (rsf_d3d11_state*)calloc(1, sizeof(*state));
    if (!state) return NULL;
    if (!rsf_d3d11_state_save(context, state)) {
        free(state);
        return NULL;
    }
    return state;
}

static void restore_state(void* context, void* snapshot)
{
    rsf_d3d11_state_restore(context, (rsf_d3d11_state*)snapshot);
    free(snapshot);
}

static const rsf_sr_d3d11_foreign foreign = {default_view, guarded_log, save_state, restore_state};

const rsf_sr_d3d11* rsf_anomaly_sr_d3d11_platform(void)
{
    return rsf_anomaly_sr_d3d11_register(&foreign);
}
