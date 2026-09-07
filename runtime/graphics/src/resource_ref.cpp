// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/resource_ref.h>

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>
#include <unknwn.h>

extern "C" void rsf_resource_retain(void* resource)
{
    if (resource) {
        static_cast<IUnknown*>(resource)->AddRef();
    }
}

extern "C" void rsf_resource_release(void* resource)
{
    if (resource) {
        static_cast<IUnknown*>(resource)->Release();
    }
}

extern "C" void* rsf_swapchain_back_buffer(void* swapchain)
{
    if (!swapchain) {
        return nullptr;
    }
    ID3D11Texture2D* buffer = nullptr;
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetBuffer(
            0, __uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&buffer)))) {
        return nullptr;
    }
    return buffer;
}
