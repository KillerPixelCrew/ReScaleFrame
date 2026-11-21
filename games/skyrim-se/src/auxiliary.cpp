// SPDX-License-Identifier: MIT
#include "auxiliary.h"
#include "observer_platform.h"
#include "resolution.h"
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdio>

extern "C" {
struct rsf_skyrim_auxiliary_texture {
    void* texture;
    uint32_t width, height, format;
};
struct rsf_skyrim_auxiliary_observation {
    void* texture;
    void* rtv;
    void* srv;
    const char* name;
    uintptr_t capture;
    uint32_t index, selector;
    bool describe;
};
struct rsf_skyrim_auxiliary_leaves {
    bool (*ready)();
    bool (*enter)();
    bool (*leave)();
    uint32_t (*thread)();
    void (*hold)(void*, rsf_skyrim_auxiliary_texture*);
    void (*release)(void*);
    bool (*upscale)(uintptr_t, void*, void*, const rsf_d3d11_auxiliary_pass*);
    bool (*restore)(uintptr_t, void*, const rsf_d3d11_auxiliary_scope*);
    void (*report)(const rsf_game_host_services*, uint64_t);
    void (*observe)(void*, const rsf_game_render_pass*, const rsf_game_host_services*, const rsf_skyrim_auxiliary_observation*);
};
bool rsf_skyrim_auxiliaries_pending() noexcept;
bool rsf_skyrim_auxiliaries_apply(const rsf_skyrim_creation_platform*, const rsf_skyrim_auxiliary_leaves*,
    void*, const rsf_game_render_pass*, const rsf_game_host_services*) noexcept;
void rsf_skyrim_auxiliaries_finish(const rsf_skyrim_auxiliary_leaves*, const rsf_game_host_services*) noexcept;
void rsf_skyrim_auxiliaries_observe(const rsf_skyrim_creation_platform*, const rsf_skyrim_auxiliary_leaves*,
    void*, const rsf_game_render_pass*, const rsf_game_host_services*) noexcept;
}

static_assert(sizeof(rsf_d3d11_auxiliary_surface) == 80 && offsetof(rsf_d3d11_auxiliary_surface, flags) == 72);
static_assert(sizeof(rsf_d3d11_auxiliary_pass) == 32 && offsetof(rsf_d3d11_auxiliary_pass, surfaces) == 24);
static_assert(sizeof(rsf_d3d11_auxiliary_scope) == 32 && sizeof(rsf_skyrim_auxiliary_texture) == 24);
static_assert(sizeof(rsf_skyrim_auxiliary_observation) == 56 && offsetof(rsf_skyrim_auxiliary_observation, describe) == 48);
static_assert(sizeof(rsf_skyrim_auxiliary_leaves) == 80);
static_assert(RSF_D3D11_AUXILIARY_DEPTH_STENCIL == 0 && RSF_D3D11_AUXILIARY_COLOUR == 1 &&
    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS == 1);
static_assert(DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_FORMAT_R16G16_FLOAT == 34 && DXGI_FORMAT_R8_UNORM == 61);

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
using Upscale = decltype(&rsf_d3d11_host_upscale_auxiliaries);
using Restore = decltype(&rsf_d3d11_host_restore_auxiliaries);
using Capture = decltype(&rsf_d3d11_host_capture_texture);

void report_restore(const rsf_game_host_services* host, uint64_t frame) noexcept try {
    const auto platform = creation_platform();
    const auto base = platform->main_module();
    ID3D11DeviceContext* context{};
    float ratios[4]{};
    if (!host->log || !base || !platform->read(base + 0x3331f30, &context, sizeof(context)) || !context ||
        !platform->read(base + 0x33364b4, ratios, sizeof(ratios))) return;
    D3D11_VIEWPORT viewport{}; UINT count = 1; context->RSGetViewports(&count, &viewport);
    char message[240]{}; std::snprintf(message, sizeof(message),
        "Skyrim auxiliary scope restored after native frame %llu; final viewport=%gx%g ratios=%g,%g,%g,%g before producer-state restoration",
        static_cast<unsigned long long>(frame), double(viewport.Width), double(viewport.Height),
        double(ratios[0]), double(ratios[1]), double(ratios[2]), double(ratios[3]));
    host->log(host->user, message);
} catch (...) {}

void observe(void* context, const rsf_game_render_pass* pass, const rsf_game_host_services* host,
    const rsf_skyrim_auxiliary_observation* facts) noexcept try {
    ComPtr<ID3D11Texture2D> texture = static_cast<ID3D11Texture2D*>(facts->texture);
    ComPtr<ID3D11RenderTargetView> target = static_cast<ID3D11RenderTargetView*>(facts->rtv);
    ComPtr<ID3D11ShaderResourceView> view = static_cast<ID3D11ShaderResourceView*>(facts->srv);
    // Native mip aliases can have views without a texture pointer in this slot.
    // GetResource supplies the allocation only. It cannot prove which mip was drawn.
    if (!texture) {
        ComPtr<ID3D11Resource> resource;
        if (view) view->GetResource(&resource);
        else if (target) target->GetResource(&resource);
        if (resource) (void)resource.As(&texture);
    }
    if (!texture) return;
    if (facts->describe && host->log) {
        D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
        D3D11_RENDER_TARGET_VIEW_DESC r{}; if (target) target->GetDesc(&r);
        D3D11_SHADER_RESOURCE_VIEW_DESC s{}; if (view) view->GetDesc(&s);
        char message[480]{};
        std::snprintf(message, sizeof(message),
            "Skyrim auxiliary %s target=%u selector=%u native_texture=%p allocation=%p size=%ux%u format=%u mips=%u slices=%u samples=%u bind=0x%x RTV=%u:%u:mip%u SRV=%u:%u:mip%u+%u native_frame=%llu; descriptor only, active coverage unverified",
            facts->name, facts->index, facts->selector, facts->texture, static_cast<void*>(texture.Get()), d.Width, d.Height, unsigned(d.Format),
            d.MipLevels, d.ArraySize, d.SampleDesc.Count, d.BindFlags, unsigned(r.Format), unsigned(r.ViewDimension),
            r.Texture2D.MipSlice, unsigned(s.Format), unsigned(s.ViewDimension), s.Texture2D.MostDetailedMip, s.Texture2D.MipLevels,
            static_cast<unsigned long long>(pass->native_frame));
        host->log(host->user, message);
    }
    if (facts->capture) {
        const rsf_d3d11_host_capture request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION,
            pass->session_id, pass->native_frame, pass->view_key, facts->name, "Native pre-HDR auxiliary allocation", texture.Get(), 0, 1};
        (void)reinterpret_cast<Capture>(facts->capture)(host->user, context, &request);
    }
} catch (...) {}

constexpr rsf_skyrim_auxiliary_leaves leaves{
    []() noexcept { return output_resolution_ready(); },
    []() noexcept { return enter_output_resolution(); },
    []() noexcept { return leave_output_resolution(); },
    []() noexcept { return uint32_t(GetCurrentThreadId()); },
    [](void* texture, rsf_skyrim_auxiliary_texture* facts) noexcept {
        auto* resource = static_cast<ID3D11Texture2D*>(texture);
        resource->AddRef();
        D3D11_TEXTURE2D_DESC desc{};
        resource->GetDesc(&desc);
        *facts = {resource, desc.Width, desc.Height, uint32_t(desc.Format)};
    },
    [](void* texture) noexcept { static_cast<ID3D11Texture2D*>(texture)->Release(); },
    [](uintptr_t service, void* user, void* context, const rsf_d3d11_auxiliary_pass* request) noexcept {
        try { return reinterpret_cast<Upscale>(service)(user, context, request) == RSF_OK; }
        catch (...) { return false; }
    },
    [](uintptr_t service, void* user, const rsf_d3d11_auxiliary_scope* scope) noexcept {
        try { return reinterpret_cast<Restore>(service)(user, scope) == RSF_OK; }
        catch (...) { return false; }
    },
    report_restore,
    observe,
};
}

bool auxiliaries_pending() { return rsf_skyrim_auxiliaries_pending(); }
bool reconstruct_auxiliaries(void* context, const rsf_game_render_pass& pass, const rsf_game_host_services& host)
{
    return rsf_skyrim_auxiliaries_apply(creation_platform(), &leaves, context, &pass, &host);
}
void finish_auxiliaries(const rsf_game_host_services& host) { rsf_skyrim_auxiliaries_finish(&leaves, &host); }
void observe_auxiliaries(void* context, const rsf_game_render_pass& pass, const rsf_game_host_services& host)
{
    rsf_skyrim_auxiliaries_observe(creation_platform(), &leaves, context, &pass, &host);
}
}
