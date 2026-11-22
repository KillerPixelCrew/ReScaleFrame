// SPDX-License-Identifier: MIT

#include <rescaleframe/d3d11_observer.h>

#include <windows.h>

#include <d3d11.h>
#include <dxgi1_2.h>

#include "d3d11_observer_callbacks.h"
#include "d3d11_observer_hooks.h"
#include "d3d11_observer_present.h"

namespace {

using present_fn = HRESULT(STDMETHODCALLTYPE*)(void*, UINT, UINT);
using present1_fn = HRESULT(STDMETHODCALLTYPE*)(void*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);

template<class Callback, class... Args>
void invoke(Callback callback, Args... args) noexcept
{
    try { callback(args...); }
    catch (...) { /* Keep foreign exceptions on the native side. */ }
}

} // namespace

extern "C" const rsf_observer11_callbacks rsf_observer11_callback_guards{
    invoke<rsf_dump_log_fn, void*, const char*>,
    invoke<rsf_observer_layout_fn, void*, void*, const rsf_observer_layout_element*, uint32_t, uint32_t>,
    invoke<rsf_observer_shader_fn, void*, void*, uint32_t, const void*, uint32_t>,
    [](rsf_observer_texture_fn callback, void* user, void* texture, const rsf_observer11_texture_facts* facts) noexcept {
        invoke(callback, user, texture, facts->width, facts->height, facts->format, facts->mip_levels,
            facts->array_size, facts->sample_count, facts->bind_flags, facts->misc_flags);
    },
    invoke<rsf_observer_buffer_fn, void*, void*, const void*, uint32_t, uint32_t>,
    invoke<rsf_observer_present_fn, void*, void*>,
    invoke<rsf_observer_present_event_fn, void*, const rsf_observer_present_event*>,
};

static_assert(D3D11_BIND_RENDER_TARGET == 0x20 && D3D11_BIND_CONSTANT_BUFFER == 0x4);
static_assert(RSF_OBSERVER_OK == 0 && RSF_OBSERVER_ERROR_INVALID_ARGUMENT == -1 && RSF_OBSERVER_ERROR_NOT_READY == -6);
static_assert(sizeof(rsf_observer11_texture_facts) == 32 && sizeof(rsf_observer11_callbacks) == 7 * sizeof(void*));
static_assert(RSF_OBSERVER_ERROR_ALREADY_INSTALLED == -3 && RSF_OBSERVER_ERROR_PATCH_FAILED == -5);

extern "C" int32_t rsf_observer11_guard_texture_extent(void* texture, uint32_t* width, uint32_t* height) noexcept try
{
    D3D11_TEXTURE2D_DESC desc{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&desc);
    *width = desc.Width;
    *height = desc.Height;
    return 1;
} catch (...) { return 0; }

static_assert(DXGI_PRESENT_TEST == 1 && S_OK == 0);
static_assert(sizeof(rsf_observer11_present_ops) == 3 * sizeof(void*));

extern "C" const rsf_observer11_present_ops rsf_observer11_present_platform{
    [](void* raw, uint32_t* width, uint32_t* height) noexcept -> int32_t {
        try {
            DXGI_SWAP_CHAIN_DESC desc{};
            if (FAILED(static_cast<IDXGISwapChain*>(raw)->GetDesc(&desc))) return 0;
            *width = desc.BufferDesc.Width;
            *height = desc.BufferDesc.Height;
            return 1;
        } catch (...) { return 0; }
    },
    [](void* raw, void** device) noexcept -> int32_t {
        try {
            return static_cast<IDXGISwapChain*>(raw)->GetDevice(__uuidof(ID3D11Device), device);
        } catch (...) { return E_FAIL; }
    },
    [](const rsf_observer_present_event* event, const void* parameters, void* target) noexcept -> int32_t {
        try {
            if (event->method == 1) {
                return reinterpret_cast<present1_fn>(target)(event->swapchain, event->sync_interval,
                    event->flags, static_cast<const DXGI_PRESENT_PARAMETERS*>(parameters));
            }
            return reinterpret_cast<present_fn>(target)(event->swapchain, event->sync_interval, event->flags);
        } catch (...) { return E_FAIL; }
    },
};

extern "C" void rsf_observer11_guard_probe_create(uint32_t driver, uint32_t feature_level,
    const DXGI_SWAP_CHAIN_DESC* desc, rsf_observer11_probe_attempt* out) noexcept
{
    IDXGISwapChain* chain{};
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    const auto wanted = static_cast<D3D_FEATURE_LEVEL>(feature_level);
    D3D_FEATURE_LEVEL obtained{};
    HRESULT result = E_FAIL;
    uint32_t returned = 0;
    try {
        result = D3D11CreateDeviceAndSwapChain(nullptr, static_cast<D3D_DRIVER_TYPE>(driver), nullptr, 0,
            &wanted, 1, D3D11_SDK_VERSION, desc, &chain, &device, &obtained, &context);
        returned = 1;
    } catch (...) {}
    *out = {context, chain, device, result, returned};
}

extern "C" uint32_t rsf_observer11_probe_chain1(void* chain, void** extended, int32_t* result) noexcept try
{
    *result = static_cast<IDXGISwapChain*>(chain)->QueryInterface(__uuidof(IDXGISwapChain1), extended);
    return 1;
} catch (...) { return 0; }

// Contain exceptions in the native registration code before returning to Rust.
extern "C" rsf_observer_result rsf_observer_install_guarded(const rsf_observer_options* options) noexcept try
{
    return rsf_observer_install(options);
} catch (...) { return RSF_OBSERVER_ERROR_NO_DEVICE; }

// The Rust thunk and its SDK call both permit unwinding to this native frame.
extern "C" int32_t rsf_observer12_try(void (*call)(void*), void* context) noexcept try
{
    call(context);
    return 1;
} catch (...) { return 0; }
