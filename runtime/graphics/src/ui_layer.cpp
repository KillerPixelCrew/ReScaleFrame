/* SPDX-License-Identifier: MIT */
#include <rescaleframe/ui_layer.h>

#include <rescaleframe/log.h>

#include <d3d11.h>
#include <wrl/client.h>

struct rsf_ui_layer_platform {
    void* (*create_texture)(void*, const rsf_ui_layer_setup*, uint32_t);
    void (*create_views)(void*, void*, uint32_t, void**, void**);
    void* (*immediate_context)(void*);
    void (*clear_target)(void*, void*);
    void (*log_ready)(const rsf_ui_layer_setup*, uint32_t);
};

namespace {

void* create_texture(void* raw_device, const rsf_ui_layer_setup* setup, uint32_t shareable)
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    D3D11_TEXTURE2D_DESC description{};
    description.Width = setup->width;
    description.Height = setup->height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    // Eight bits of alpha preserve partial UI coverage. Typeless allows the sRGB RTV when needed.
    description.Format = setup->srgb ? DXGI_FORMAT_R8G8B8A8_TYPELESS : DXGI_FORMAT_R8G8B8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags =
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    if (shareable) {
        description.MiscFlags = D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE;
    }
    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &texture))) {
        return nullptr;
    }
    return texture.Detach();
}

void create_views(void* raw_device, void* raw_texture, uint32_t srgb, void** target, void** source)
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto* texture = static_cast<ID3D11Texture2D*>(raw_texture);
    D3D11_RENDER_TARGET_VIEW_DESC target_desc{};
    D3D11_SHADER_RESOURCE_VIEW_DESC source_desc{};
    if (srgb) {
        // Encode on the diverted draw and read the stored bits for the composite.
        target_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
        target_desc.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
        source_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        source_desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        source_desc.Texture2D.MipLevels = 1;
    }
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> rtv;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
    if (FAILED(device->CreateRenderTargetView(texture, srgb ? &target_desc : nullptr, &rtv)) ||
        !rtv ||
        FAILED(device->CreateShaderResourceView(texture, srgb ? &source_desc : nullptr, &srv)) ||
        !srv) {
        return;
    }
    *target = rtv.Detach();
    *source = srv.Detach();
}

void* immediate_context(void* device)
{
    ID3D11DeviceContext* context = nullptr;
    static_cast<ID3D11Device*>(device)->GetImmediateContext(&context);
    return context;
}

void clear_target(void* context, void* target)
{
    const FLOAT transparent[4] = {};
    static_cast<ID3D11DeviceContext*>(context)->ClearRenderTargetView(
        static_cast<ID3D11RenderTargetView*>(target), transparent);
}

void log_ready(const rsf_ui_layer_setup* setup, uint32_t shareable)
{
    rsf::say(setup->log, setup->log_user, "ui layer: %ux%u R8G8B8A8_UNORM, %u deep, %s",
             setup->width, setup->height, static_cast<unsigned>(RSF_UI_LAYER_RING),
             shareable ? "shareable" : "not shared");
}

constexpr rsf_ui_layer_platform platform{create_texture, create_views, immediate_context,
                                         clear_target, log_ready};
static_assert(sizeof(rsf_ui_layer_platform) == 5 * sizeof(void*));
static_assert(RSF_UI_LAYER_OK == 0 && RSF_UI_LAYER_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_UI_LAYER_ERROR_ABI_MISMATCH == -2 && RSF_UI_LAYER_ERROR_RESOURCE_FAILED == -3);

} // namespace

extern "C" rsf_ui_layer_result rsf_core_ui_layer_create(void*, const rsf_ui_layer_setup*,
                                                       rsf_ui_layer**,
                                                       const rsf_ui_layer_platform*);

extern "C" rsf_ui_layer_result rsf_ui_layer_create(void* device, const rsf_ui_layer_setup* setup,
                                                   rsf_ui_layer** out)
{
    return rsf_core_ui_layer_create(device, setup, out, &platform);
}
