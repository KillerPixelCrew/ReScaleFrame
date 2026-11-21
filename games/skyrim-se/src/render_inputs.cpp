// SPDX-License-Identifier: MIT
#include "render_inputs.h"
#include "auxiliary.h"
#include "cpu_events.h"
#include "observer_platform.h"
#include "presentation_inputs.h"
#include "resolution.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstdio>

extern "C" {
struct rsf_skyrim_texture_facts {
    uintptr_t device;
    uint32_t width, height, samples, slices, mips, format;
};
struct rsf_skyrim_viewport_report {
    uint32_t render[2], output[2];
    float ratios[2];
    uint32_t lock;
};
struct rsf_skyrim_render_camera_leaves {
    bool (*extent)(uint32_t, uint32_t, uint32_t*, uint32_t*);
    bool (*clock)(int64_t*, int64_t*);
};
struct rsf_skyrim_render_input_leaves {
    rsf_skyrim_render_camera_leaves camera;
    void (*reference)(void*, bool);
    void* (*context_device)(void*);
    uintptr_t (*chain_device)(void*);
    void (*texture)(void*, rsf_skyrim_texture_facts*);
    uint32_t (*thread)();
    bool (*identity)(rsf_game_render_pass*);
    bool (*eligible)();
    void (*diagnostic)(const rsf_game_render_pass*);
    void (*viewport)(void*, const rsf_game_host_services*, const rsf_skyrim_viewport_report*);
    bool (*upscale)(void*, const rsf_game_render_pass*, const rsf_game_host_services*);
    bool (*auxiliaries)(void*, const rsf_game_render_pass*, const rsf_game_host_services*);
    void (*observe)(void*, const rsf_game_render_pass*, const rsf_game_host_services*);
    bool (*publish)(void*, const rsf_game_render_pass*, const rsf_game_host_services*);
};
bool rsf_skyrim_render_inputs(const rsf_skyrim_creation_platform*, const rsf_skyrim_render_input_leaves*,
    uintptr_t manager, const rsf_game_host_services*, bool fg) noexcept;
uint32_t rsf_skyrim_renderer_thread() noexcept;
}

static_assert(sizeof(rsf_skyrim_texture_facts) == 32 && sizeof(rsf_skyrim_viewport_report) == 28);
static_assert(sizeof(rsf_skyrim_render_camera_leaves) == 16 && sizeof(rsf_skyrim_render_input_leaves) == 120);
static_assert(RSF_SCREEN_GAMEPLAY == 8);
static_assert(DXGI_FORMAT_R11G11B10_FLOAT == 26 && DXGI_FORMAT_R24G8_TYPELESS == 44);
static_assert(DXGI_FORMAT_R8G8_UNORM == 49 && DXGI_FORMAT_R8G8_SNORM == 51 &&
    DXGI_FORMAT_R8G8B8A8_UNORM == 28 && DXGI_FORMAT_R8G8B8A8_SNORM == 31 &&
    DXGI_FORMAT_R16G16_FLOAT == 34 && DXGI_FORMAT_R16G16_UNORM == 35 && DXGI_FORMAT_R16G16_SNORM == 37 &&
    DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R16G16B16A16_UNORM == 11 &&
    DXGI_FORMAT_R16G16B16A16_SNORM == 13 && DXGI_FORMAT_R32G32_FLOAT == 16 && DXGI_FORMAT_R32G32B32A32_FLOAT == 2);

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;

constexpr rsf_skyrim_render_input_leaves leaves{
    {
        [](uint32_t w, uint32_t h, uint32_t* rw, uint32_t* rh) noexcept { return resolution_extent(w, h, *rw, *rh); },
        [](int64_t* tick, int64_t* rate) noexcept {
            LARGE_INTEGER now{}, frequency{};
            if (!QueryPerformanceCounter(&now) || !QueryPerformanceFrequency(&frequency)) return false;
            *tick = now.QuadPart;
            *rate = frequency.QuadPart;
            return true;
        },
    },
    [](void* raw, bool retain) noexcept {
        auto* object = static_cast<IUnknown*>(raw);
        if (retain) object->AddRef();
        else object->Release();
    },
    [](void* raw) noexcept -> void* {
        auto* context = static_cast<ID3D11DeviceContext*>(raw);
        if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return nullptr;
        ComPtr<ID3D11Device> device;
        context->GetDevice(&device);
        return device.Detach();
    },
    [](void* raw) noexcept -> uintptr_t {
        ComPtr<ID3D11Device> device;
        if (FAILED(static_cast<IDXGISwapChain*>(raw)->GetDevice(IID_PPV_ARGS(&device)))) return 0;
        return uintptr_t(device.Get());
    },
    [](void* raw, rsf_skyrim_texture_facts* facts) noexcept {
        auto* texture = static_cast<ID3D11Texture2D*>(raw);
        ComPtr<ID3D11Device> device;
        D3D11_TEXTURE2D_DESC d{};
        texture->GetDevice(&device);
        texture->GetDesc(&d);
        *facts = {uintptr_t(device.Get()), d.Width, d.Height, d.SampleDesc.Count, d.ArraySize, d.MipLevels, uint32_t(d.Format)};
    },
    []() noexcept { return uint32_t(GetCurrentThreadId()); },
    [](rsf_game_render_pass* pass) noexcept { try { return cpu_render_identity(*pass); } catch (...) { return false; } },
    cpu_rendered_source_eligible,
    [](const rsf_game_render_pass* pass) noexcept { try { cpu_render_diagnostic("FG_INPUTS", *pass); } catch (...) {} },
    [](void* raw, const rsf_game_host_services* host, const rsf_skyrim_viewport_report* report) noexcept {
        D3D11_VIEWPORT viewport{}; UINT viewports = 1;
        static_cast<ID3D11DeviceContext*>(raw)->RSGetViewports(&viewports, &viewport);
        char message[240]{}; std::snprintf(message, sizeof(message),
            "Skyrim pre-HDR native viewport %.5gx%.5g (%u views); active inputs %ux%u allocation %ux%u lock=%u ratios=%g,%g",
            double(viewport.Width), double(viewport.Height), viewports, report->render[0], report->render[1], report->output[0], report->output[1],
            report->lock, double(report->ratios[0]), double(report->ratios[1]));
        try { host->log(host->user, message); } catch (...) {}
    },
    [](void* context, const rsf_game_render_pass* pass, const rsf_game_host_services* host) noexcept {
        try { return host->process_upscale(host->user, context, pass) == RSF_OK; } catch (...) { return false; }
    },
    [](void* context, const rsf_game_render_pass* pass, const rsf_game_host_services* host) noexcept {
        try { return reconstruct_auxiliaries(context, *pass, *host); } catch (...) { return false; }
    },
    [](void* context, const rsf_game_render_pass* pass, const rsf_game_host_services* host) noexcept {
        try { observe_auxiliaries(context, *pass, *host); } catch (...) {}
    },
    [](void* context, const rsf_game_render_pass* pass, const rsf_game_host_services* host) noexcept {
        try {
            host->render_pass(host->user, context, pass, 1);
            host->render_pass(host->user, context, pass, 0);
            return true;
        } catch (...) { return false; }
    },
};
}

uint32_t renderer_thread() { return rsf_skyrim_renderer_thread(); }
bool reconstruct(void* manager, const rsf_game_host_services& host)
{
    return rsf_skyrim_render_inputs(creation_platform(), &leaves, uintptr_t(manager), &host, false);
}
bool prepare_presentation_inputs(void* manager, const rsf_game_host_services& host)
{
    return rsf_skyrim_render_inputs(creation_platform(), &leaves, uintptr_t(manager), &host, true);
}
}
