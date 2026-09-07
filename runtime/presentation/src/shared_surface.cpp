/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/shared_surface.h>

#include <windows.h>

#include <d3d11_4.h>
#include <d3d12.h>

#include <cstdarg>
#include <cstdio>
#include <new>

namespace {

void say(rsf_shared_log_fn log, void* user, const char* format, ...)
{
    char message[512];
    va_list arguments;
    if (!log) {
        return;
    }
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    log(user, message);
}

/* Typeless formats are refused rather than resolved. A shared surface is opened by a runtime that
   cannot ask what was intended, and picking an interpretation on its behalf is how a colour buffer
   quietly becomes a depth buffer. */
bool format_is_typeless(uint32_t format)
{
    switch (format) {
    case DXGI_FORMAT_R32G32B32A32_TYPELESS:
    case DXGI_FORMAT_R32G32B32_TYPELESS:
    case DXGI_FORMAT_R16G16B16A16_TYPELESS:
    case DXGI_FORMAT_R32G32_TYPELESS:
    case DXGI_FORMAT_R10G10B10A2_TYPELESS:
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:
    case DXGI_FORMAT_R16G16_TYPELESS:
    case DXGI_FORMAT_R32_TYPELESS:
    case DXGI_FORMAT_R24G8_TYPELESS:
    case DXGI_FORMAT_R8G8_TYPELESS:
    case DXGI_FORMAT_R16_TYPELESS:
    case DXGI_FORMAT_R8_TYPELESS:
    case DXGI_FORMAT_BC1_TYPELESS:
    case DXGI_FORMAT_BC2_TYPELESS:
    case DXGI_FORMAT_BC3_TYPELESS:
    case DXGI_FORMAT_B8G8R8A8_TYPELESS:
    case DXGI_FORMAT_B8G8R8X8_TYPELESS:
        return true;
    default:
        return false;
    }
}

} // namespace

struct rsf_shared_surface {
    ID3D11Texture2D* texture = nullptr;
    ID3D11RenderTargetView* target = nullptr;
    ID3D12Resource* opened = nullptr;
    HANDLE handle = nullptr;
};

struct rsf_shared_fence {
    ID3D11Fence* fence11 = nullptr;
    ID3D12Fence* fence12 = nullptr;
    HANDLE handle = nullptr;
};

extern "C" rsf_shared_result rsf_shared_surface_create(void* d3d11_device_pointer,
                                                       void* d3d12_device_pointer,
                                                       const rsf_shared_surface_setup* setup,
                                                       rsf_shared_surface** out)
{
    if (!d3d11_device_pointer || !setup || !out ||
        setup->struct_size < sizeof(rsf_shared_surface_setup)) {
        return RSF_SHARED_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_SHARED_SURFACE_ABI_VERSION) {
        return RSF_SHARED_ERROR_ABI_MISMATCH;
    }
    if (setup->width == 0 || setup->height == 0 || setup->format == 0 ||
        format_is_typeless(setup->format)) {
        return RSF_SHARED_ERROR_INVALID_ARGUMENT;
    }
    *out = nullptr;

    auto* device = static_cast<ID3D11Device*>(d3d11_device_pointer);
    auto* surface = new (std::nothrow) rsf_shared_surface();
    if (!surface) {
        return RSF_SHARED_ERROR_CREATE_FAILED;
    }

    D3D11_TEXTURE2D_DESC description{};
    description.Width = setup->width;
    description.Height = setup->height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = static_cast<DXGI_FORMAT>(setup->format);
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    if (setup->render_target) {
        description.BindFlags |= D3D11_BIND_RENDER_TARGET;
    }
    /* Both flags together. `SHARED_NTHANDLE` alone is not a thing D3D11 accepts, and `SHARED`
       alone produces a handle only another D3D11 device can open. */
    description.MiscFlags = D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE;

    HRESULT made = device->CreateTexture2D(&description, nullptr, &surface->texture);
    if (FAILED(made) || !surface->texture) {
        say(setup->log, setup->log_user,
            "shared surface: %ux%u format %u could not be created shareable, hr 0x%08lx",
            setup->width, setup->height, setup->format, (unsigned long)made);
        rsf_shared_surface_destroy(surface);
        return RSF_SHARED_ERROR_CREATE_FAILED;
    }

    if (setup->render_target) {
        if (FAILED(device->CreateRenderTargetView(surface->texture, nullptr, &surface->target))) {
            rsf_shared_surface_destroy(surface);
            return RSF_SHARED_ERROR_CREATE_FAILED;
        }
    }

    /* The handle comes from IDXGIResource1, not from the texture. A runtime that supports the flags
       but not the interface fails here rather than at open time, which is a clearer place for it. */
    IDXGIResource1* resource = nullptr;
    made = surface->texture->QueryInterface(__uuidof(IDXGIResource1),
                                            reinterpret_cast<void**>(&resource));
    if (FAILED(made) || !resource) {
        say(setup->log, setup->log_user,
            "shared surface: the texture is not an IDXGIResource1, hr 0x%08lx", (unsigned long)made);
        rsf_shared_surface_destroy(surface);
        return RSF_SHARED_ERROR_NO_HANDLE;
    }
    made = resource->CreateSharedHandle(nullptr, DXGI_SHARED_RESOURCE_READ |
                                                     DXGI_SHARED_RESOURCE_WRITE,
                                        nullptr, &surface->handle);
    resource->Release();
    if (FAILED(made) || !surface->handle) {
        say(setup->log, setup->log_user, "shared surface: no NT handle, hr 0x%08lx",
            (unsigned long)made);
        rsf_shared_surface_destroy(surface);
        return RSF_SHARED_ERROR_NO_HANDLE;
    }

    if (d3d12_device_pointer) {
        auto* device12 = static_cast<ID3D12Device*>(d3d12_device_pointer);
        made = device12->OpenSharedHandle(surface->handle, __uuidof(ID3D12Resource),
                                          reinterpret_cast<void**>(&surface->opened));
        if (FAILED(made) || !surface->opened) {
            /* The interesting failure. The handle exists, so the D3D11 side is willing; the D3D12
               side will not take it. On Windows that is a bug, and under Proton it is a statement
               about whether DXVK and vkd3d-proton agree about memory. */
            say(setup->log, setup->log_user,
                "shared surface: D3D12 would not open the handle, hr 0x%08lx", (unsigned long)made);
            rsf_shared_surface_destroy(surface);
            return RSF_SHARED_ERROR_OPEN_FAILED;
        }
    }

    *out = surface;
    return RSF_SHARED_OK;
}

extern "C" void rsf_shared_surface_destroy(rsf_shared_surface* surface)
{
    if (!surface) {
        return;
    }
    if (surface->opened) {
        surface->opened->Release();
    }
    if (surface->target) {
        surface->target->Release();
    }
    if (surface->texture) {
        surface->texture->Release();
    }
    /* The handle is closed after the resources that were opened from it. Closing it first is legal
       and makes a leak look like a driver problem, which is a bad half hour. */
    if (surface->handle) {
        CloseHandle(surface->handle);
    }
    delete surface;
}

extern "C" void* rsf_shared_surface_d3d11(rsf_shared_surface* surface)
{
    return surface ? surface->texture : nullptr;
}

extern "C" void* rsf_shared_surface_d3d12(rsf_shared_surface* surface)
{
    return surface ? surface->opened : nullptr;
}

extern "C" void* rsf_shared_surface_target(rsf_shared_surface* surface)
{
    return surface ? surface->target : nullptr;
}

extern "C" rsf_shared_result rsf_shared_fence_create(void* d3d11_device_pointer,
                                                     void* d3d12_device_pointer,
                                                     rsf_shared_log_fn log, void* log_user,
                                                     rsf_shared_fence** out)
{
    if (!d3d11_device_pointer || !out) {
        return RSF_SHARED_ERROR_INVALID_ARGUMENT;
    }
    *out = nullptr;

    /* ID3D11Device5 rather than ID3D11Device: fences are an 11.4 feature and a runtime without them
       cannot bridge at all, so failing to get the interface is the same answer as failing to make
       the fence. */
    ID3D11Device5* device = nullptr;
    HRESULT made = static_cast<ID3D11Device*>(d3d11_device_pointer)
                       ->QueryInterface(__uuidof(ID3D11Device5), reinterpret_cast<void**>(&device));
    if (FAILED(made) || !device) {
        say(log, log_user, "shared fence: no ID3D11Device5, hr 0x%08lx", (unsigned long)made);
        return RSF_SHARED_ERROR_CREATE_FAILED;
    }

    auto* fence = new (std::nothrow) rsf_shared_fence();
    if (!fence) {
        device->Release();
        return RSF_SHARED_ERROR_CREATE_FAILED;
    }

    made = device->CreateFence(0, D3D11_FENCE_FLAG_SHARED, __uuidof(ID3D11Fence),
                               reinterpret_cast<void**>(&fence->fence11));
    device->Release();
    if (FAILED(made) || !fence->fence11) {
        say(log, log_user, "shared fence: could not be created, hr 0x%08lx", (unsigned long)made);
        rsf_shared_fence_destroy(fence);
        return RSF_SHARED_ERROR_CREATE_FAILED;
    }

    made = fence->fence11->CreateSharedHandle(nullptr, GENERIC_ALL, nullptr, &fence->handle);
    if (FAILED(made) || !fence->handle) {
        say(log, log_user, "shared fence: no NT handle, hr 0x%08lx", (unsigned long)made);
        rsf_shared_fence_destroy(fence);
        return RSF_SHARED_ERROR_NO_HANDLE;
    }

    if (d3d12_device_pointer) {
        made = static_cast<ID3D12Device*>(d3d12_device_pointer)
                   ->OpenSharedHandle(fence->handle, __uuidof(ID3D12Fence),
                                      reinterpret_cast<void**>(&fence->fence12));
        if (FAILED(made) || !fence->fence12) {
            say(log, log_user, "shared fence: D3D12 would not open the handle, hr 0x%08lx",
                (unsigned long)made);
            rsf_shared_fence_destroy(fence);
            return RSF_SHARED_ERROR_OPEN_FAILED;
        }
    }

    *out = fence;
    return RSF_SHARED_OK;
}

extern "C" void rsf_shared_fence_destroy(rsf_shared_fence* fence)
{
    if (!fence) {
        return;
    }
    if (fence->fence12) {
        fence->fence12->Release();
    }
    if (fence->fence11) {
        fence->fence11->Release();
    }
    if (fence->handle) {
        CloseHandle(fence->handle);
    }
    delete fence;
}

extern "C" void* rsf_shared_fence_d3d11(rsf_shared_fence* fence)
{
    return fence ? fence->fence11 : nullptr;
}

extern "C" void* rsf_shared_fence_d3d12(rsf_shared_fence* fence)
{
    return fence ? fence->fence12 : nullptr;
}
