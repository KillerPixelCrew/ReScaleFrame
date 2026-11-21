// SPDX-License-Identifier: MIT
#include "device_owner.h"

#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/log.h>
#include <rescaleframe/scene_promote.h>
#include <rescaleframe/srv_format.h>

#include <wrl/client.h>

struct rsf_promote_resources {
    ID3D11Texture2D* texture;
    ID3D11RenderTargetView* target;
    ID3D11ShaderResourceView* view;
    uint32_t format;
};

struct rsf_promote_platform {
    void (*make_replacement)(const rsf_promote_setup*, void*, uint32_t, const char*, rsf_promote_resources*);
    void* (*make_view)(void*, void*, uint32_t);
    uint32_t (*copy)(void*, void*, void*, void*);
    rsf_fullscreen_pass* (*create_blit)(void*);
    uint32_t (*draw_blit)(rsf_fullscreen_pass*, void*, void*, void*, uint32_t, uint32_t);
    void (*destroy_blit)(rsf_fullscreen_pass*);
    void (*log)(rsf_promote_log_fn, void*, const char*);
};

static_assert(sizeof(rsf_promote_resources) == 32 &&
    sizeof(rsf_promote_platform) == 7 * sizeof(void*));
static_assert(sizeof(rsf_promote_setup) == 40 && sizeof(rsf_promote_frame_tail) == 144 &&
    sizeof(rsf_promote_status) == 36 && sizeof(rsf_frame_tap_plan) == 424);
static_assert(RSF_PROMOTE_OK == 0 && RSF_PROMOTE_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_PROMOTE_ERROR_ABI_MISMATCH == -2 && RSF_PROMOTE_ERROR_RESOURCE_FAILED == -3 &&
    RSF_PROMOTE_ERROR_NOT_SCALED == -4);

namespace {
// The typeless family a format belongs to, or the format itself when it has none.
DXGI_FORMAT typeless_family(DXGI_FORMAT format)
{
    switch (format) {
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:
    case DXGI_FORMAT_R8G8B8A8_UINT:
    case DXGI_FORMAT_R8G8B8A8_SNORM:
    case DXGI_FORMAT_R8G8B8A8_SINT:
        return DXGI_FORMAT_R8G8B8A8_TYPELESS;
    case DXGI_FORMAT_B8G8R8A8_TYPELESS:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:
        return DXGI_FORMAT_B8G8R8A8_TYPELESS;
    case DXGI_FORMAT_B8G8R8X8_TYPELESS:
    case DXGI_FORMAT_B8G8R8X8_UNORM:
    case DXGI_FORMAT_B8G8R8X8_UNORM_SRGB:
        return DXGI_FORMAT_B8G8R8X8_TYPELESS;
    case DXGI_FORMAT_R10G10B10A2_TYPELESS:
    case DXGI_FORMAT_R10G10B10A2_UNORM:
    case DXGI_FORMAT_R10G10B10A2_UINT:
        return DXGI_FORMAT_R10G10B10A2_TYPELESS;
    case DXGI_FORMAT_R16G16B16A16_TYPELESS:
    case DXGI_FORMAT_R16G16B16A16_FLOAT:
    case DXGI_FORMAT_R16G16B16A16_UNORM:
    case DXGI_FORMAT_R16G16B16A16_UINT:
    case DXGI_FORMAT_R16G16B16A16_SNORM:
    case DXGI_FORMAT_R16G16B16A16_SINT:
        return DXGI_FORMAT_R16G16B16A16_TYPELESS;
    default:
        return format;
    }
}

DXGI_FORMAT typed_view_format(DXGI_FORMAT texture_format, uint32_t hint)
{
    // A hint from another surface's family is no hint: the chain's hint was measured on one
    // candidate and applied to a later one of a different family, and the view failed to create
    // on every rebuild. The family is the texture's; the hint only settles sRGB or not within it.
    if (hint != 0 &&
        typeless_family(static_cast<DXGI_FORMAT>(hint)) == typeless_family(texture_format)) {
        return static_cast<DXGI_FORMAT>(hint);
    }
    // srv_format does not know the X8 family, which promotion has always resolved to UNORM.
    if (texture_format == DXGI_FORMAT_B8G8R8X8_TYPELESS) {
        return DXGI_FORMAT_B8G8R8X8_UNORM;
    }
    return rsf::srv_format(texture_format);
}

void make_replacement(const rsf_promote_setup* setup, void* raw, uint32_t hint, const char* role,
                      rsf_promote_resources* out) try
{
    auto* device = static_cast<ID3D11Device*>(setup->device);
    auto* original = static_cast<ID3D11Texture2D*>(raw);
    if (!rsf::same_device(original, device)) {
        rsf::say(setup->log, setup->log_user, "promote: the %s belongs to another device", role);
        return;
    }

    D3D11_TEXTURE2D_DESC original_desc{};
    original->GetDesc(&original_desc);
    rsf::say(setup->log, setup->log_user,
             "promote: the %s %p, %ux%u format %u, becomes %ux%u", role, raw,
             original_desc.Width, original_desc.Height, unsigned(original_desc.Format),
             setup->output_width, setup->output_height);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = setup->output_width;
    desc.Height = setup->output_height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = original_desc.Format;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;

    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &texture)) || !texture) {
        rsf::say(setup->log, setup->log_user, "promote: the %s replacement could not be created", role);
        return;
    }
    const auto format = typed_view_format(desc.Format, hint);
    D3D11_RENDER_TARGET_VIEW_DESC target_desc{};
    target_desc.Format = format;
    target_desc.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
    D3D11_SHADER_RESOURCE_VIEW_DESC view_desc{};
    view_desc.Format = format;
    view_desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    view_desc.Texture2D.MipLevels = 1;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> target;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
    if (FAILED(device->CreateRenderTargetView(texture.Get(), &target_desc, &target)) || !target ||
        FAILED(device->CreateShaderResourceView(texture.Get(), &view_desc, &view)) || !view) {
        rsf::say(setup->log, setup->log_user,
                 "promote: the %s replacement views could not be created with view format %u",
                 role, unsigned(format));
        return;
    }
    *out = {texture.Detach(), target.Detach(), view.Detach(), uint32_t(desc.Format)};
} catch (...) {}

void* make_view(void* raw_device, void* raw_texture, uint32_t typed)
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto* texture = static_cast<ID3D11Texture2D*>(raw_texture);
    if (!rsf::same_device(texture, device)) return nullptr;
    D3D11_SHADER_RESOURCE_VIEW_DESC view_desc{};
    if (typed) {
        D3D11_TEXTURE2D_DESC desc{};
        texture->GetDesc(&desc);
        view_desc.Format = typed_view_format(desc.Format, 0);
        view_desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        view_desc.Texture2D.MipLevels = 1;
    }
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
    if (FAILED(device->CreateShaderResourceView(texture, typed ? &view_desc : nullptr, &view))) {
        return nullptr;
    }
    return view.Detach();
}

uint32_t copy(void* device, void* raw_context, void* target, void* source)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE ||
        !rsf::same_device(context, static_cast<ID3D11Device*>(device))) return 0;
    context->CopyResource(static_cast<ID3D11Texture2D*>(target), static_cast<ID3D11Texture2D*>(source));
    return 1;
}

rsf_fullscreen_pass* create_blit(void* device)
{
    rsf_fullscreen_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION;
    rsf_fullscreen_pass* pass = nullptr;
    return rsf_fullscreen_pass_create(device, &setup, &pass) == RSF_FULLSCREEN_OK ? pass : nullptr;
}

uint32_t draw_blit(rsf_fullscreen_pass* pass, void* context, void* target, void* source,
                   uint32_t width, uint32_t height)
{
    rsf_fullscreen_draw draw{};
    draw.struct_size = sizeof(draw);
    draw.mode = RSF_FULLSCREEN_COPY;
    draw.width = width;
    draw.height = height;
    return rsf_fullscreen_pass_draw(pass, context, target, source, &draw) == RSF_FULLSCREEN_OK;
}

void emit_log(rsf_promote_log_fn sink, void* user, const char* text) try
{
    if (sink) sink(user, text);
} catch (...) {}

constexpr rsf_promote_platform platform{make_replacement, make_view, copy, create_blit, draw_blit,
    rsf_fullscreen_pass_destroy, emit_log};
}

extern "C" {
rsf_promote_result rsf_core_promote_create(const rsf_promote_setup*, rsf_promote**,
    const rsf_promote_platform*);
rsf_promote_result rsf_core_promote_prepare(const rsf_promote*, const rsf_promote_frame_tail*);
rsf_promote_result rsf_core_promote_fill_plan(const rsf_promote*, rsf_frame_tap_plan*);
rsf_promote_result rsf_core_promote_seed(const rsf_promote*, void*, uint32_t);
rsf_promote_result rsf_core_promote_finish(const rsf_promote*, void*);
rsf_promote_result rsf_core_promote_get_stand_ins(const rsf_promote*, void**, void**);
rsf_promote_result rsf_core_promote_get_status(const rsf_promote*, rsf_promote_status*);
void rsf_core_promote_destroy(rsf_promote*);

rsf_promote_result rsf_promote_create(const rsf_promote_setup* setup, rsf_promote** out)
{
    return rsf_core_promote_create(setup, out, &platform);
}

rsf_promote_result rsf_promote_prepare(rsf_promote* promote, const rsf_promote_frame_tail* tail)
{
    return rsf_core_promote_prepare(promote, tail);
}

rsf_promote_result rsf_promote_fill_plan(rsf_promote* promote, rsf_frame_tap_plan* plan)
{
    return rsf_core_promote_fill_plan(promote, plan);
}

rsf_promote_result rsf_promote_seed(rsf_promote* promote, void* context, uint32_t reconstructed)
{
    return rsf_core_promote_seed(promote, context, reconstructed);
}

rsf_promote_result rsf_promote_finish(rsf_promote* promote, void* context)
{
    return rsf_core_promote_finish(promote, context);
}

rsf_promote_result rsf_promote_get_stand_ins(rsf_promote* promote, void** scene, void** composed)
{
    return rsf_core_promote_get_stand_ins(promote, scene, composed);
}

rsf_promote_result rsf_promote_get_status(rsf_promote* promote, rsf_promote_status* status)
{
    return rsf_core_promote_get_status(promote, status);
}

void rsf_promote_destroy(rsf_promote* promote)
{
    rsf_core_promote_destroy(promote);
}
}
