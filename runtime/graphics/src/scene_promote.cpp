// SPDX-License-Identifier: MIT
#include "device_owner.h"

#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/log.h>
#include <rescaleframe/scene_promote.h>
#include <rescaleframe/srv_format.h>

#include <wrl/client.h>

struct rsf_promote_texture_facts {
    uint32_t width, height, format, default_view_format;
};

struct rsf_promote_platform {
    uint32_t (*owns_texture)(void*, void*);
    uint32_t (*describe)(void*, rsf_promote_texture_facts*);
    uint32_t (*log_replacement)(const rsf_promote_setup*, void*, const char*, const rsf_promote_texture_facts*);
    void* (*make_texture)(void*, uint32_t, uint32_t, uint32_t);
    void* (*make_target)(void*, void*, uint32_t);
    void* (*make_view)(void*, void*, uint32_t, uint32_t);
    uint32_t (*copy)(void*, void*, void*, void*);
    rsf_fullscreen_pass* (*create_blit)(void*);
    uint32_t (*draw_blit)(rsf_fullscreen_pass*, void*, void*, void*, uint32_t, uint32_t);
    void (*destroy_blit)(rsf_fullscreen_pass*);
    void (*log)(rsf_promote_log_fn, void*, const char*);
};

static_assert(sizeof(rsf_promote_texture_facts) == 16 &&
    sizeof(rsf_promote_platform) == 11 * sizeof(void*));
static_assert(sizeof(rsf_promote_setup) == 40 && sizeof(rsf_promote_frame_tail) == 144 &&
    sizeof(rsf_promote_status) == 36 && sizeof(rsf_frame_tap_plan) == 424);
static_assert(RSF_PROMOTE_OK == 0 && RSF_PROMOTE_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_PROMOTE_ERROR_ABI_MISMATCH == -2 && RSF_PROMOTE_ERROR_RESOURCE_FAILED == -3 &&
    RSF_PROMOTE_ERROR_NOT_SCALED == -4);

namespace {
uint32_t owns_texture(void* device, void* texture) noexcept try
{
    return rsf::same_device(static_cast<ID3D11Texture2D*>(texture), static_cast<ID3D11Device*>(device));
} catch (...) { return 0; }

uint32_t describe(void* texture, rsf_promote_texture_facts* out) noexcept try
{
    D3D11_TEXTURE2D_DESC desc{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&desc);
    *out = {desc.Width, desc.Height, uint32_t(desc.Format), uint32_t(rsf::srv_format(desc.Format))};
    return 1;
} catch (...) { return 0; }

uint32_t log_replacement(const rsf_promote_setup* setup, void* original, const char* role,
                         const rsf_promote_texture_facts* facts) noexcept try
{
    rsf::say(setup->log, setup->log_user,
             "promote: the %s %p, %ux%u format %u, becomes %ux%u", role, original,
             facts->width, facts->height, facts->format, setup->output_width, setup->output_height);
    return 1;
} catch (...) { return 0; }

void* make_texture(void* raw_device, uint32_t width, uint32_t height, uint32_t format) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;

    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &texture))) return nullptr;
    return texture.Detach();
} catch (...) { return nullptr; }

void* make_target(void* raw_device, void* raw_texture, uint32_t format) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto* texture = static_cast<ID3D11Texture2D*>(raw_texture);
    D3D11_RENDER_TARGET_VIEW_DESC desc{};
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;

    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> target;
    if (FAILED(device->CreateRenderTargetView(texture, &desc, &target))) return nullptr;
    return target.Detach();
} catch (...) { return nullptr; }

void* make_view(void* raw_device, void* raw_texture, uint32_t typed, uint32_t format) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto* texture = static_cast<ID3D11Texture2D*>(raw_texture);
    D3D11_SHADER_RESOURCE_VIEW_DESC desc{};
    if (typed) {
        desc.Format = static_cast<DXGI_FORMAT>(format);
        desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        desc.Texture2D.MipLevels = 1;
    }

    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
    if (FAILED(device->CreateShaderResourceView(texture, typed ? &desc : nullptr, &view))) return nullptr;
    return view.Detach();
} catch (...) { return nullptr; }

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

constexpr rsf_promote_platform platform{owns_texture, describe, log_replacement, make_texture, make_target, make_view,
    copy, create_blit, draw_blit, rsf_fullscreen_pass_destroy, emit_log};
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
