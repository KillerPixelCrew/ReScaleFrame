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

extern "C" void* rsf_create_render_target_view(void* device, void* texture)
{
    if (!device || !texture) {
        return nullptr;
    }
    ID3D11RenderTargetView* view = nullptr;
    // A null descriptor means the texture's own format, which is right for a back buffer and
    // refuses outright for a typeless one rather than picking an interpretation on its behalf.
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateRenderTargetView(
            static_cast<ID3D11Resource*>(texture), nullptr, &view))) {
        return nullptr;
    }
    return view;
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
