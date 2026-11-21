// SPDX-License-Identifier: MIT
#include "device_owner.h"
#include "fullscreen_platform.h"
#include <rescaleframe/d3d11_state.h>
#include <dxgi.h>
#include <wrl/client.h>

namespace {
void* back_buffer(void* raw, uint32_t* size)
{
    auto* chain = static_cast<IDXGISwapChain*>(raw);
    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&texture))) || !texture) return nullptr;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    size[0] = desc.Width;
    size[1] = desc.Height;
    return texture.Detach();
}

void* target_view(void* raw_device, void* raw_texture)
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto* texture = static_cast<ID3D11Texture2D*>(raw_texture);
    if (!rsf::same_device(texture, device)) return nullptr;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> view;
    if (FAILED(device->CreateRenderTargetView(texture, nullptr, &view))) return nullptr;
    return view.Detach();
}

void* source_view(void* device, void* texture)
{
    return rsf::same_device(static_cast<ID3D11Texture2D*>(texture), static_cast<ID3D11Device*>(device)) ?
        rsf_d3d11_create_shader_view(device, texture) : nullptr;
}

constexpr rsf_present_blit_platform platform{back_buffer, target_view, source_view};
static_assert(sizeof(rsf_present_blit_platform) == 3 * sizeof(void*));
static_assert(sizeof(rsf_present_blit_setup) == sizeof(rsf_fullscreen_setup) && RSF_PRESENT_BLIT_ABI_VERSION == 1);
static_assert(RSF_PRESENT_BLIT_OK == 0 && RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_PRESENT_BLIT_ERROR_ABI_MISMATCH == -2 && RSF_PRESENT_BLIT_ERROR_SHADER_FAILED == -3 &&
    RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED == -4 && RSF_PRESENT_BLIT_ERROR_NO_BACK_BUFFER == -5);
}

extern "C" rsf_present_blit_result rsf_present_blit_create(
    void* device, const rsf_present_blit_setup* setup, rsf_present_blit** out)
{
    return rsf_core_present_blit_create(device, setup, out, rsf_fullscreen_native_platform());
}

extern "C" rsf_present_blit_result rsf_present_blit_draw(rsf_present_blit* blit,
    void* context, void* swapchain, void* source, uint32_t tonemap)
{
    return rsf_core_present_blit_draw(blit, context, swapchain, source, tonemap, &platform);
}

extern "C" void rsf_present_blit_destroy(rsf_present_blit* blit)
{
    // Both opaque handles contain the same Rust pass.
    rsf_core_fullscreen_destroy(reinterpret_cast<rsf_fullscreen_pass*>(blit));
}
