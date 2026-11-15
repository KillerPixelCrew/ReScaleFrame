// SPDX-License-Identifier: GPL-3.0-only
#include "resolution.h"
#include "render_inputs.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <atomic>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
std::atomic<bool> owned{};
uint32_t render_width{}, render_height{}, surface_width{}, surface_height{};
float saved_ratios[4]{}, last_ratios[2]{};
uint32_t saved_lock{};
float halton(uint32_t index, uint32_t base) {
    float value = 0, factor = 1;
    while (index) { factor /= float(base); value += factor * float(index % base); index /= base; }
    return value;
}
void reset_native_history(uintptr_t base) {
    const auto manager = *reinterpret_cast<uintptr_t*>(base + 0x33d40a0);
    if (!manager) return;
    const auto taa = *reinterpret_cast<uintptr_t*>(manager + 0x1f0);
    if (taa) *reinterpret_cast<uint32_t*>(taa + 0x30) |= 1u;
}
}
bool resolution_owned() { return owned.load(std::memory_order_acquire); }
bool resolution_extent(uint32_t width, uint32_t height, uint32_t& rw, uint32_t& rh) {
    if (!resolution_owned() || surface_width != width || surface_height != height) return false;
    rw = render_width; rh = render_height; return true;
}
void configure_resolution(void* pointer, const rsf_game_host_services& host) {
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    auto* state = static_cast<unsigned char*>(pointer);
    if (uintptr_t(pointer) != base + 0x33363a0) return;
    auto* ratios = reinterpret_cast<float*>(state + 0x114);
    auto* lock = reinterpret_cast<uint32_t*>(state + 0x128);
    const auto width = *reinterpret_cast<uint32_t*>(state + 0x24), height = *reinterpret_cast<uint32_t*>(state + 0x28);
    rsf_game_render_config config{sizeof(config)};
    if (renderer_thread() == GetCurrentThreadId() && host.process_upscale && host.render_config && width && height && width <= 16384 && height <= 16384) {
        const auto window = *reinterpret_cast<uintptr_t*>(base + 0x3330198);
        auto* native_context = *reinterpret_cast<ID3D11DeviceContext**>(base + 0x3331f30);
        auto* native_chain = window ? *reinterpret_cast<IDXGISwapChain**>(window + 0x18) : nullptr;
        if (native_context && native_chain) {
            ComPtr<ID3D11DeviceContext> context = native_context;
            ComPtr<IDXGISwapChain> chain = native_chain;
            rsf_game_render_pass prepare{}; prepare.struct_size = sizeof(prepare); prepare.role = RSF_GAME_RENDER_SR;
            prepare.session_id = host.session_id; prepare.flags = RSF_GAME_RENDER_RECONFIGURE | RSF_GAME_RENDER_NO_FG;
            prepare.swapchain = chain.Get(); prepare.camera.output_width = width; prepare.camera.output_height = height;
            (void)host.process_upscale(host.user, context.Get(), &prepare);
        }
        (void)host.render_config(host.user, &config);
    }
    const bool enabled = config.enabled && config.output_width == width && config.output_height == height &&
        config.render_width && config.render_height && config.render_width <= width && config.render_height <= height;
    if (!enabled) {
        if (owned.exchange(false, std::memory_order_acq_rel)) {
            std::memcpy(ratios, saved_ratios, sizeof(saved_ratios)); *lock = saved_lock;
            reset_native_history(base);
            if (host.log) host.log(host.user, "Skyrim native resolution ownership released; original ratios restored.");
        }
        return;
    }
    const bool acquired = !owned.load(std::memory_order_relaxed);
    if (acquired) { std::memcpy(saved_ratios, ratios, sizeof(saved_ratios)); saved_lock = *lock; }
    const bool changed = acquired || render_width != config.render_width || render_height != config.render_height || surface_width != width || surface_height != height;
    ratios[2] = acquired ? ratios[0] : last_ratios[0]; ratios[3] = acquired ? ratios[1] : last_ratios[1];
    ratios[0] = float(config.render_width) / float(width); ratios[1] = float(config.render_height) / float(height);
    last_ratios[0] = ratios[0]; last_ratios[1] = ratios[1]; *lock = 0;
    render_width = config.render_width; render_height = config.render_height; surface_width = width; surface_height = height;
    // Native viewport/cache consumers select these ratios only in their unlocked main-view scope.
    // Auxiliary callers retain the native lock; their additional exceptions need separate proof.
    const auto frame = *reinterpret_cast<uint32_t*>(base + 0x33363f4);
    const auto phases = std::clamp(uint32_t(8.0 * double(width) * width / (double(render_width) * render_width)), 8u, 256u);
    const float jitter[]{-2 * (halton(frame % phases + 1, 2) - .5f) / float(render_width),
        2 * (halton(frame % phases + 1, 3) - .5f) / float(render_height)};
    std::memcpy(state + 0x44, jitter, sizeof(jitter)); std::memcpy(state + 0x4c, jitter, sizeof(jitter));
    owned.store(true, std::memory_order_release);
    if (changed) {
        reset_native_history(base);
        if (host.log) {
            char message[220]{}; std::snprintf(message, sizeof(message),
                "Skyrim native resolution plan %ux%u -> %ux%u ratios=%g,%g phases=%u saved_lock=%u; reduced SR admission awaits auxiliary-buffer repair",
                render_width, render_height, width, height, double(ratios[0]), double(ratios[1]), phases, saved_lock);
            host.log(host.user, message);
        }
    }
}
}
