// SPDX-License-Identifier: MIT
#include "resolution.h"
#include "observer_platform.h"
#include "render_inputs.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>

extern "C" {
struct rsf_skyrim_resolution_report {
    uint32_t render[2], output[2];
    float ratios[2];
    uint32_t phases, saved_lock;
};
struct rsf_skyrim_resolution_upload {
    uintptr_t renderer, dirty, upload, viewport;
    uint32_t extent[2];
    uint8_t flags;
};
struct rsf_skyrim_resolution_leaves {
    bool (*renderer_thread)();
    uint32_t (*thread)();
    void (*prepare)(const rsf_game_host_services*, void*, const rsf_game_render_pass*);
    void (*config)(const rsf_game_host_services*, rsf_game_render_config*);
    void (*ratios)(uintptr_t, const float*);
    void (*lock)(uintptr_t, uint32_t);
    void (*jitter)(uintptr_t, const float*);
    void (*upload)(const rsf_skyrim_resolution_upload*);
    void (*report)(const rsf_game_host_services*, const rsf_skyrim_resolution_report*);
};
void rsf_skyrim_resolution_configure(const rsf_skyrim_creation_platform*, const rsf_skyrim_resolution_leaves*,
    uintptr_t, const rsf_game_host_services*) noexcept;
bool rsf_skyrim_resolution_owned() noexcept;
bool rsf_skyrim_resolution_controls(uintptr_t) noexcept;
bool rsf_skyrim_resolution_extent(uint32_t, uint32_t, uint32_t*, uint32_t*) noexcept;
void rsf_skyrim_resolution_record_flags(uint8_t) noexcept;
bool rsf_skyrim_resolution_output_ready() noexcept;
bool rsf_skyrim_resolution_enter(const rsf_skyrim_creation_platform*, const rsf_skyrim_resolution_leaves*) noexcept;
bool rsf_skyrim_resolution_leave(const rsf_skyrim_resolution_leaves*) noexcept;
}

static_assert(sizeof(rsf_skyrim_resolution_report) == 32 && sizeof(rsf_skyrim_resolution_leaves) == 72);
static_assert(sizeof(rsf_skyrim_resolution_upload) == 48 && offsetof(rsf_skyrim_resolution_upload, flags) == 40);

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
using Upload = void (*)(void*, uint8_t);
using Viewport = uintptr_t (*)(void*, uint32_t, uint32_t, uint8_t);

constexpr rsf_skyrim_resolution_leaves leaves{
    []() noexcept { return renderer_thread() == GetCurrentThreadId(); },
    []() noexcept { return uint32_t(GetCurrentThreadId()); },
    [](const rsf_game_host_services* host, void* raw_context, const rsf_game_render_pass* pass) noexcept {
        ComPtr<ID3D11DeviceContext> context = static_cast<ID3D11DeviceContext*>(raw_context);
        ComPtr<IDXGISwapChain> chain = static_cast<IDXGISwapChain*>(pass->swapchain);
        auto borrowed = *pass;
        borrowed.swapchain = chain.Get();
        try { (void)host->process_upscale(host->user, context.Get(), &borrowed); } catch (...) {}
    },
    [](const rsf_game_host_services* host, rsf_game_render_config* config) noexcept {
        try { (void)host->render_config(host->user, config); }
        catch (...) { *config = {sizeof(*config)}; }
    },
    [](uintptr_t address, const float* ratios) noexcept { std::memcpy(reinterpret_cast<void*>(address), ratios, 4 * sizeof(float)); },
    [](uintptr_t address, uint32_t value) noexcept { *reinterpret_cast<uint32_t*>(address) = value; },
    [](uintptr_t address, const float* jitter) noexcept {
        constexpr auto bytes = 2 * sizeof(float);
        std::memcpy(reinterpret_cast<void*>(address), jitter, bytes);
        std::memcpy(reinterpret_cast<void*>(address + bytes), jitter, bytes);
    },
    [](const rsf_skyrim_resolution_upload* args) noexcept {
        *reinterpret_cast<unsigned char*>(args->dirty) = 1;
        auto* renderer = reinterpret_cast<void*>(args->renderer);
        reinterpret_cast<Upload>(args->upload)(renderer, args->flags);
        reinterpret_cast<Viewport>(args->viewport)(renderer, args->extent[0], args->extent[1], 0);
    },
    [](const rsf_game_host_services* host, const rsf_skyrim_resolution_report* plan) noexcept {
        char message[220]{}; std::snprintf(message, sizeof(message),
            "Skyrim native resolution plan %ux%u -> %ux%u ratios=%g,%g phases=%u saved_lock=%u; native auxiliary handoff required for reduced SR",
            plan->render[0], plan->render[1], plan->output[0], plan->output[1],
            double(plan->ratios[0]), double(plan->ratios[1]), plan->phases, plan->saved_lock);
        try { host->log(host->user, message); } catch (...) {}
    },
};
}

bool resolution_owned() { return rsf_skyrim_resolution_owned(); }
bool resolution_controls(void* state) { return rsf_skyrim_resolution_controls(uintptr_t(state)); }
void configure_resolution(void* state, const rsf_game_host_services& host)
{
    rsf_skyrim_resolution_configure(creation_platform(), &leaves, uintptr_t(state), &host);
}
bool resolution_extent(uint32_t width, uint32_t height, uint32_t& rw, uint32_t& rh)
{
    return rsf_skyrim_resolution_extent(width, height, &rw, &rh);
}
void record_per_frame_flags(uint8_t flags) { rsf_skyrim_resolution_record_flags(flags); }
bool output_resolution_ready() { return rsf_skyrim_resolution_output_ready(); }
bool enter_output_resolution() { return rsf_skyrim_resolution_enter(creation_platform(), &leaves); }
bool leave_output_resolution() { return rsf_skyrim_resolution_leave(&leaves); }
}
