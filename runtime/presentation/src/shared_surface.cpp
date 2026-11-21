// SPDX-License-Identifier: MIT
#include <rescaleframe/shared_surface.h>
#include <d3d11_4.h>
#include <d3d12.h>
#include <cstddef>

using Create = HRESULT (*)(void*, void**) noexcept;
using Open = HRESULT (*)(void*, void*, void**) noexcept;

struct rsf_shared_surface_platform {
    HRESULT (*texture)(void*, const rsf_shared_surface_setup*, void**) noexcept;
    Open target;
    Create query_resource;
    Create resource_handle;
    Open open_resource;
    Create query_device5;
    Create fence;
    Create fence_handle;
    Open open_fence;
};

namespace {
using Platform = rsf_shared_surface_platform;

static_assert(RSF_SHARED_SURFACE_ABI_VERSION == 2 && RSF_SHARED_OK == 0 &&
    RSF_SHARED_ERROR_INVALID_ARGUMENT == -1 && RSF_SHARED_ERROR_ABI_MISMATCH == -2 &&
    RSF_SHARED_ERROR_CREATE_FAILED == -3 && RSF_SHARED_ERROR_NO_HANDLE == -4 &&
    RSF_SHARED_ERROR_OPEN_FAILED == -5);
static_assert(sizeof(rsf_shared_surface_setup) == 48 && offsetof(rsf_shared_surface_setup, log) == 24 &&
    offsetof(rsf_shared_surface_setup, unordered_access) == 40 && sizeof(Platform) == 9 * sizeof(void*));
static_assert(DXGI_FORMAT_R32G8X24_TYPELESS == 19 && DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS == 21 &&
    DXGI_FORMAT_X32_TYPELESS_G8X24_UINT == 22 && DXGI_FORMAT_R24_UNORM_X8_TYPELESS == 46 &&
    DXGI_FORMAT_X24_TYPELESS_G8_UINT == 47 && DXGI_FORMAT_BC4_TYPELESS == 79 &&
    DXGI_FORMAT_BC5_TYPELESS == 82 && DXGI_FORMAT_BC6H_TYPELESS == 94 && DXGI_FORMAT_BC7_TYPELESS == 97);

HRESULT texture(void* device, const rsf_shared_surface_setup* setup, void** out) noexcept
{
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
    if (setup->unordered_access) {
        description.BindFlags |= D3D11_BIND_UNORDERED_ACCESS;
    }
    description.MiscFlags = D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE;

    ID3D11Texture2D* value = nullptr;
    const auto hr = static_cast<ID3D11Device*>(device)->CreateTexture2D(&description, nullptr, &value);
    *out = value;
    return hr;
}

HRESULT target(void* device, void* texture11, void** out) noexcept
{
    ID3D11RenderTargetView* value = nullptr;
    const auto hr = static_cast<ID3D11Device*>(device)->CreateRenderTargetView(
        static_cast<ID3D11Texture2D*>(texture11), nullptr, &value);
    *out = value;
    return hr;
}

template<class Interface, class Object>
HRESULT query(void* object, void** out) noexcept
{
    Interface* value = nullptr;
    const auto hr = static_cast<Object*>(object)->QueryInterface(IID_PPV_ARGS(&value));
    *out = value;
    return hr;
}

HRESULT resource_handle(void* resource, void** out) noexcept
{
    return static_cast<IDXGIResource1*>(resource)->CreateSharedHandle(
        nullptr, DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE, nullptr, out);
}

HRESULT fence(void* device, void** out) noexcept
{
    ID3D11Fence* value = nullptr;
    const auto hr = static_cast<ID3D11Device5*>(device)->CreateFence(
        0, D3D11_FENCE_FLAG_SHARED, IID_PPV_ARGS(&value));
    *out = value;
    return hr;
}

HRESULT fence_handle(void* fence11, void** out) noexcept
{
    return static_cast<ID3D11Fence*>(fence11)->CreateSharedHandle(nullptr, GENERIC_ALL, nullptr, out);
}

template<class Interface>
HRESULT open(void* device, void* handle, void** out) noexcept
{
    Interface* value = nullptr;
    const auto hr = static_cast<ID3D12Device*>(device)->OpenSharedHandle(handle, IID_PPV_ARGS(&value));
    *out = value;
    return hr;
}

constexpr Platform platform{texture, target, query<IDXGIResource1, ID3D11Texture2D>,
    resource_handle, open<ID3D12Resource>, query<ID3D11Device5, ID3D11Device>,
    fence, fence_handle, open<ID3D12Fence>};
}

extern "C" const rsf_shared_surface_platform* rsf_shared_surface_native_platform()
{
    return &platform;
}

extern "C" rsf_shared_result rsf_core_shared_surface_create(const Platform*, void*, void*,
    const rsf_shared_surface_setup*, rsf_shared_surface**);
extern "C" rsf_shared_result rsf_core_shared_fence_create(const Platform*, void*, void*,
    rsf_shared_log_fn, void*, rsf_shared_fence**);

extern "C" rsf_shared_result rsf_shared_surface_create(void* device11, void* device12,
    const rsf_shared_surface_setup* setup, rsf_shared_surface** out)
{
    return rsf_core_shared_surface_create(&platform, device11, device12, setup, out);
}

extern "C" rsf_shared_result rsf_shared_fence_create(void* device11, void* device12,
    rsf_shared_log_fn log, void* user, rsf_shared_fence** out)
{
    return rsf_core_shared_fence_create(&platform, device11, device12, log, user, out);
}
