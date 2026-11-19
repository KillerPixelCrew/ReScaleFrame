// SPDX-License-Identifier: MIT
#include <rescaleframe/native_scene.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>

namespace {
uint64_t resource_identity(void* resource)
{
    Microsoft::WRL::ComPtr<IUnknown> identity;
    if (!resource || FAILED(static_cast<IUnknown*>(resource)->QueryInterface(IID_PPV_ARGS(&identity)))) return 0;
    return uint64_t(reinterpret_cast<uintptr_t>(identity.Get()));
}

uint64_t backbuffer_identity(void* swapchain, uint32_t* width, uint32_t* height)
{
    Microsoft::WRL::ComPtr<ID3D11Texture2D> buffer;
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetBuffer(0, IID_PPV_ARGS(&buffer)))) return 0;
    D3D11_TEXTURE2D_DESC desc{};
    buffer->GetDesc(&desc);
    *width = desc.Width;
    *height = desc.Height;
    return resource_identity(buffer.Get());
}
}

extern "C" int rsf_core_native_scene_pass(const rsf_game_render_pass*, uint32_t begin,
    decltype(&resource_identity));
extern "C" int rsf_core_native_scene_texture_binding(const rsf_game_render_pass*, decltype(&resource_identity));
extern "C" uint32_t rsf_core_native_scene_present_path(const rsf_game_render_pass*, decltype(&backbuffer_identity));

extern "C" int rsf_native_scene_pass(const rsf_game_render_pass* pass, uint32_t begin)
{
    return rsf_core_native_scene_pass(pass, begin, resource_identity);
}

extern "C" int rsf_native_scene_texture_binding(const rsf_game_render_pass* pass)
{
    return rsf_core_native_scene_texture_binding(pass, resource_identity);
}

extern "C" uint32_t rsf_native_scene_present_path(const rsf_game_render_pass* window)
{
    return rsf_core_native_scene_present_path(window, backbuffer_identity);
}
