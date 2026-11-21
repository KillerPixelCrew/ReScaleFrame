// SPDX-License-Identifier: MIT
#include "render_inputs.h"
#include "render_camera.h"
#include "observer_platform.h"
#include "resolution.h"
#include "auxiliary.h"
#include "presentation_inputs.h"
#include "cpu_events.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstring>
#include <cstdio>
#include <initializer_list>
#include <atomic>

struct rsf_skyrim_render_camera_leaves {
    bool (*extent)(uint32_t, uint32_t, uint32_t*, uint32_t*);
    bool (*clock)(int64_t*, int64_t*);
};
static_assert(sizeof(rsf_skyrim_render_camera_leaves) == 16);
extern "C" bool rsf_skyrim_read_render_camera(const rsf_skyrim_creation_platform*,
    const rsf_skyrim_render_camera_leaves*, uintptr_t manager, bool fg,
    rsf_game_render_pass*, rsf::skyrim::RenderCamera*) noexcept;

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
std::atomic<uint32_t> scene_thread{};
bool read(void* out, uintptr_t address, size_t size) noexcept {
#if defined(_MSC_VER)
    __try { std::memcpy(out, reinterpret_cast<const void*>(address), size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION m{};
    if (!VirtualQuery(reinterpret_cast<void*>(address), &m, sizeof(m)) || m.State != MEM_COMMIT ||
        (m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) || address + size > uintptr_t(m.BaseAddress) + m.RegionSize) return false;
    std::memcpy(out, reinterpret_cast<const void*>(address), size); return true;
#endif
}
template<class T> bool read(T& out, uintptr_t address) { return read(&out, address, sizeof(out)); }
bool temporal_mask_format(DXGI_FORMAT format) {
    // TAA consumes numeric X/Y rejection channels, never colour or integer data.
    switch (format) {
    case DXGI_FORMAT_R8G8_UNORM: case DXGI_FORMAT_R8G8_SNORM:
    case DXGI_FORMAT_R8G8B8A8_UNORM: case DXGI_FORMAT_R8G8B8A8_SNORM:
    case DXGI_FORMAT_R16G16_FLOAT: case DXGI_FORMAT_R16G16_UNORM: case DXGI_FORMAT_R16G16_SNORM:
    case DXGI_FORMAT_R16G16B16A16_FLOAT: case DXGI_FORMAT_R16G16B16A16_UNORM: case DXGI_FORMAT_R16G16B16A16_SNORM:
    case DXGI_FORMAT_R32G32_FLOAT: case DXGI_FORMAT_R32G32B32A32_FLOAT:
        return true;
    default: return false;
    }
}
}
bool read_render_camera(void* manager, bool fg, rsf_game_render_pass& pass, RenderCamera& facts) noexcept
{
    constexpr rsf_skyrim_render_camera_leaves leaves{
        [](uint32_t w, uint32_t h, uint32_t* rw, uint32_t* rh) noexcept { return resolution_extent(w, h, *rw, *rh); },
        [](int64_t* tick, int64_t* rate) noexcept {
            LARGE_INTEGER now{}, frequency{};
            if (!QueryPerformanceCounter(&now) || !QueryPerformanceFrequency(&frequency)) return false;
            *tick = now.QuadPart;
            *rate = frequency.QuadPart;
            return true;
        },
    };
    return rsf_skyrim_read_render_camera(creation_platform(), &leaves, uintptr_t(manager), fg, &pass, &facts);
}

uint32_t renderer_thread() { return scene_thread.load(std::memory_order_acquire); }
bool reconstruct(void* manager, const rsf_game_host_services& host) {
    if (!host.process_upscale) return false;
    rsf_game_render_pass pass{};
    RenderCamera view{};
    if (!read_render_camera(manager, false, pass, view)) return false;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const auto [width, height] = view.output;
    const auto [render_width, render_height] = view.render;
    const auto camera = view.camera, window = view.window;
    const auto frame = view.frame;
    ID3D11DeviceContext* raw_context{}; ID3D11Texture2D *raw_color{}, *raw_depth{}, *raw_motion{}, *raw_mask{}; IDXGISwapChain* raw_chain{};
    if (!read(raw_context, base + 0x3331f30) || !raw_context ||
        !read(raw_color, base + 0x3331f40 + 0xa58 + 0x30) || !raw_color ||
        !read(raw_motion, base + 0x3331f40 + 0xa58 + 7 * 0x30) || !raw_motion ||
        !read(raw_depth, base + 0x3333f58) || !raw_depth || !read(raw_chain, window + 0x18) || !raw_chain) return false;
    ComPtr<ID3D11DeviceContext> context = raw_context;
    ComPtr<ID3D11Texture2D> color = raw_color, depth = raw_depth, motion = raw_motion, mask;
    if (read(raw_mask, base + 0x3331f40 + 0xa58 + 85 * 0x30) && raw_mask) mask = raw_mask;
    ComPtr<IDXGISwapChain> chain = raw_chain;
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return false;
    static uint32_t logged_width = 0, logged_height = 0;
    if (host.log && (logged_width != render_width || logged_height != render_height)) {
        D3D11_VIEWPORT viewport{}; UINT viewports = 1; context->RSGetViewports(&viewports, &viewport);
        char message[240]{}; std::snprintf(message, sizeof(message),
            "Skyrim pre-HDR native viewport %.5gx%.5g (%u views); active inputs %ux%u allocation %ux%u lock=%u ratios=%g,%g",
            double(viewport.Width), double(viewport.Height), viewports, render_width, render_height, width, height,
            *reinterpret_cast<const uint32_t*>(base + 0x33364c8), double(view.ratios[0]), double(view.ratios[1]));
        host.log(host.user, message); logged_width = render_width; logged_height = render_height;
    }
    ComPtr<ID3D11Device> device, owner; context->GetDevice(&device);
    for (auto* texture : {color.Get(), depth.Get(), motion.Get()}) {
        texture->GetDevice(&owner); if (owner.Get() != device.Get()) return false;
        D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
        if (d.Width != width || d.Height != height || d.SampleDesc.Count != 1 || d.ArraySize != 1 || d.MipLevels != 1) return false;
    }
    if (FAILED(chain->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get() != device.Get()) return false;
    D3D11_TEXTURE2D_DESC c{}, d{}, m{}; color->GetDesc(&c); depth->GetDesc(&d); motion->GetDesc(&m);
    if ((c.Format != DXGI_FORMAT_R11G11B10_FLOAT && c.Format != DXGI_FORMAT_R16G16B16A16_FLOAT) ||
        d.Format != DXGI_FORMAT_R24G8_TYPELESS || m.Format != DXGI_FORMAT_R16G16_FLOAT) return false;
    if (mask) {
        D3D11_TEXTURE2D_DESC t{}; mask->GetDesc(&t); mask->GetDevice(&owner);
        if (owner.Get() != device.Get() || t.Width != width || t.Height != height ||
            t.SampleDesc.Count != 1 || t.ArraySize != 1 || t.MipLevels != 1 || !temporal_mask_format(t.Format)) mask.Reset();
    }
    scene_thread.store(GetCurrentThreadId(), std::memory_order_release);
    pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR; pass.session_id = host.session_id;
    pass.family_key = pass.view_key = pass.history_key = camera; pass.pass_key = 0x153ade7; pass.native_frame = frame;
    pass.flags |= RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG;
    if (host.render_pass) (void)cpu_render_identity(pass);
    pass.flags |= RSF_GAME_RENDER_NO_FG; // SR metadata alone does not lease FG inputs.
    pass.viewport_key = pass.window_key = pass.rhi_viewport_key = window;
    pass.render_rect[2] = int32_t(render_width); pass.render_rect[3] = int32_t(render_height);
    pass.output_rect[2] = int32_t(width); pass.output_rect[3] = int32_t(height);
    pass.color_input = pass.color_output = color.Get(); pass.depth = depth.Get(); pass.motion = motion.Get();
    // Snapshot during this synchronous lease, before native postprocessing can reuse target85.
    pass.temporal_mask = mask.Get();
    if (mask) { pass.temporal_mask_weights[0] = 0.1f; pass.temporal_mask_weights[1] = 1; }
    pass.swapchain = chain.Get(); pass.camera_valid = 1;
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = -1; pass.motion_camera_included = 1;
    const bool reduced = render_width != width || render_height != height;
    bool reconstructed = false;
    if (reduced) reconstructed = reconstruct_auxiliaries(context.Get(), pass, host);
    else reconstructed = host.process_upscale(host.user, context.Get(), &pass) == RSF_OK;
    observe_auxiliaries(context.Get(), pass, host);
    identify_presentation_inputs(pass);
    return reconstructed;
}
}
