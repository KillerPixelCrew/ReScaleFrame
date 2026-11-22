/* SPDX-License-Identifier: MIT */
/* Contain foreign C++ exceptions around the D3D11 and D3D12 observer's SDK and callback calls.
   MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the same
   exceptions the former catch-all stopped. Access violations and other faults still propagate. */
#include <rescaleframe/d3d11_observer.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#define COBJMACROS
#include <windows.h>
#include <initguid.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <stddef.h>

#include "d3d11_observer_callbacks.h"
#include "d3d11_observer_hooks.h"
#include "d3d11_observer_present.h"

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL
#define RSF_FOREIGN_EXCEPTION \
    (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH)

typedef HRESULT (STDMETHODCALLTYPE* present_fn)(void*, UINT, UINT);
typedef HRESULT (STDMETHODCALLTYPE* present1_fn)(void*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);

/* Keep foreign exceptions on the native side. */
static void guard_log(rsf_dump_log_fn callback, void* user, const char* text)
{
    __try { callback(user, text); } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guard_layout(rsf_observer_layout_fn callback, void* user, void* object,
    const rsf_observer_layout_element* elements, uint32_t copied, uint32_t count)
{
    __try { callback(user, object, elements, copied, count); } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guard_shader(rsf_observer_shader_fn callback, void* user, void* object, uint32_t stage,
    const void* bytes, uint32_t size)
{
    __try { callback(user, object, stage, bytes, size); } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guard_texture(rsf_observer_texture_fn callback, void* user, void* texture,
    const rsf_observer11_texture_facts* facts)
{
    __try {
        callback(user, texture, facts->width, facts->height, facts->format, facts->mip_levels,
            facts->array_size, facts->sample_count, facts->bind_flags, facts->misc_flags);
    } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guard_buffer(rsf_observer_buffer_fn callback, void* user, void* object, const void* bytes,
    uint32_t size, uint32_t flags)
{
    __try { callback(user, object, bytes, size, flags); } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guard_present(rsf_observer_present_fn callback, void* user, void* chain)
{
    __try { callback(user, chain); } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guard_event(rsf_observer_present_event_fn callback, void* user,
    const rsf_observer_present_event* event)
{
    __try { callback(user, event); } __except (RSF_FOREIGN_EXCEPTION) {}
}

const rsf_observer11_callbacks rsf_observer11_callback_guards = {
    guard_log, guard_layout, guard_shader, guard_texture, guard_buffer, guard_present, guard_event,
};

_Static_assert(D3D11_BIND_RENDER_TARGET == 0x20 && D3D11_BIND_CONSTANT_BUFFER == 0x4, "D3D11 bind flags");
_Static_assert(RSF_OBSERVER_OK == 0 && RSF_OBSERVER_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_OBSERVER_ERROR_NOT_READY == -6, "observer result codes");
_Static_assert(sizeof(rsf_observer11_texture_facts) == 32 &&
    sizeof(rsf_observer11_callbacks) == 7 * sizeof(void*), "observer callback layout");
_Static_assert(RSF_OBSERVER_ERROR_ALREADY_INSTALLED == -3 && RSF_OBSERVER_ERROR_PATCH_FAILED == -5,
    "observer result codes");

int32_t rsf_observer11_guard_texture_extent(void* texture, uint32_t* width, uint32_t* height)
{
    __try {
        D3D11_TEXTURE2D_DESC desc = {0};
        ID3D11Texture2D_GetDesc((ID3D11Texture2D*)texture, &desc);
        *width = desc.Width;
        *height = desc.Height;
        return 1;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return 0;
    }
}

_Static_assert(DXGI_PRESENT_TEST == 1 && S_OK == 0, "DXGI present constants");
_Static_assert(sizeof(rsf_observer11_present_ops) == 3 * sizeof(void*), "observer present ops layout");

static int32_t present_describe(void* raw, uint32_t* width, uint32_t* height)
{
    __try {
        DXGI_SWAP_CHAIN_DESC desc = {0};
        if (FAILED(IDXGISwapChain_GetDesc((IDXGISwapChain*)raw, &desc))) return 0;
        *width = desc.BufferDesc.Width;
        *height = desc.BufferDesc.Height;
        return 1;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return 0;
    }
}

static int32_t present_device(void* raw, void** device)
{
    __try {
        return IDXGISwapChain_GetDevice((IDXGISwapChain*)raw, &IID_ID3D11Device, device);
    } __except (RSF_FOREIGN_EXCEPTION) {
        return E_FAIL;
    }
}

static int32_t present_forward(const rsf_observer_present_event* event, const void* parameters, void* target)
{
    __try {
        if (event->method == 1) {
            return ((present1_fn)target)(event->swapchain, event->sync_interval, event->flags,
                (const DXGI_PRESENT_PARAMETERS*)parameters);
        }
        return ((present_fn)target)(event->swapchain, event->sync_interval, event->flags);
    } __except (RSF_FOREIGN_EXCEPTION) {
        return E_FAIL;
    }
}

const rsf_observer11_present_ops rsf_observer11_present_platform = {
    present_describe, present_device, present_forward,
};

void rsf_observer11_guard_probe_create(uint32_t driver, uint32_t feature_level,
    const DXGI_SWAP_CHAIN_DESC* desc, rsf_observer11_probe_attempt* out)
{
    const D3D_FEATURE_LEVEL wanted = (D3D_FEATURE_LEVEL)feature_level;
    D3D_FEATURE_LEVEL obtained = 0;
    /* Outputs written before an exception stay visible so the caller can release them. */
    out->context = NULL;
    out->chain = NULL;
    out->device = NULL;
    out->result = E_FAIL;
    out->returned = 0;
    __try {
        out->result = D3D11CreateDeviceAndSwapChain(NULL, (D3D_DRIVER_TYPE)driver, NULL, 0, &wanted, 1,
            D3D11_SDK_VERSION, desc, (IDXGISwapChain**)&out->chain, (ID3D11Device**)&out->device, &obtained,
            (ID3D11DeviceContext**)&out->context);
        out->returned = 1;
    } __except (RSF_FOREIGN_EXCEPTION) {}
}

uint32_t rsf_observer11_probe_chain1(void* chain, void** extended, int32_t* result)
{
    __try {
        *result = IDXGISwapChain_QueryInterface((IDXGISwapChain*)chain, &IID_IDXGISwapChain1, extended);
        return 1;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return 0;
    }
}

/* Contain exceptions in the native registration code before returning to Rust. */
rsf_observer_result rsf_observer_install_guarded(const rsf_observer_options* options)
{
    __try {
        return rsf_observer_install(options);
    } __except (RSF_FOREIGN_EXCEPTION) {
        return RSF_OBSERVER_ERROR_NO_DEVICE;
    }
}

/* The Rust thunk and its SDK call both permit unwinding to this native frame. */
int32_t rsf_observer12_try(void (*call)(void*), void* context)
{
    __try {
        call(context);
        return 1;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return 0;
    }
}
