// SPDX-License-Identifier: MIT
#include "resolution.h"
#include "render_inputs.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <atomic>
#include <algorithm>
#include <cstdio>
#include <cstring>

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
std::atomic<bool> owned{};
uint32_t render_width{}, render_height{}, surface_width{}, surface_height{};
float saved_ratios[4]{}, last_ratios[2]{};
uint32_t saved_lock{};
bool output_active{};
float producer_ratios[4]{};
thread_local bool have_per_frame_flags{};
thread_local uint8_t per_frame_flags{};
float halton(uint32_t index, uint32_t base) {
    float value = 0, factor = 1;
    while (index) { factor /= float(base); value += factor * float(index % base); index /= base; }
    return value;
}
template<class T> bool read(T& value, uintptr_t address) {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address),
        &value, sizeof(value), &copied) && copied == sizeof(value);
}
void reset_native_history(uintptr_t base) {
    const auto manager = *reinterpret_cast<uintptr_t*>(base + 0x33d40a0);
    if (!manager) return;
    const auto taa = *reinterpret_cast<uintptr_t*>(manager + 0x1f0);
    if (taa) *reinterpret_cast<uint32_t*>(taa + 0x30) |= 1u;
}
}
bool resolution_owned() { return owned.load(std::memory_order_acquire); }
void record_per_frame_flags(uint8_t flags) { per_frame_flags = flags; have_per_frame_flags = true; }
bool output_resolution_ready() { return resolution_owned() && !output_active && have_per_frame_flags; }
bool enter_output_resolution() {
    if (!output_resolution_ready()) return false;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    auto* ratios = reinterpret_cast<float*>(base + 0x33364b4);
    std::memcpy(producer_ratios, ratios, sizeof(producer_ratios));
    for (unsigned i = 0; i < 4; ++i) ratios[i] = 1;
    *reinterpret_cast<unsigned char*>(base + 0x3336400) = 1;
    using Upload = void (*)(void*, uint8_t);
    reinterpret_cast<Upload>(base + 0x100ae80)(reinterpret_cast<void*>(base + 0x3331f40), per_frame_flags);
    using Viewport = uintptr_t (*)(void*, uint32_t, uint32_t, uint8_t);
    reinterpret_cast<Viewport>(base + 0x1009960)(reinterpret_cast<void*>(base + 0x3331f40), surface_width, surface_height, 0);
    output_active = true; return true;
}
void leave_output_resolution() {
    if (!output_active) return;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    std::memcpy(reinterpret_cast<void*>(base + 0x33364b4), producer_ratios, sizeof(producer_ratios));
    *reinterpret_cast<unsigned char*>(base + 0x3336400) = 1;
    using Upload = void (*)(void*, uint8_t);
    reinterpret_cast<Upload>(base + 0x100ae80)(reinterpret_cast<void*>(base + 0x3331f40), per_frame_flags);
    using Viewport = uintptr_t (*)(void*, uint32_t, uint32_t, uint8_t);
    reinterpret_cast<Viewport>(base + 0x1009960)(reinterpret_cast<void*>(base + 0x3331f40), render_width, render_height, 0);
    output_active = false;
}
bool resolution_extent(uint32_t width, uint32_t height, uint32_t& rw, uint32_t& rh) {
    if (!resolution_owned() || surface_width != width || surface_height != height) return false;
    rw = render_width; rh = render_height; return true;
}
void configure_resolution(void* pointer, const rsf_game_host_services& host) {
    if (output_active) return;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    auto* state = static_cast<unsigned char*>(pointer);
    if (uintptr_t(pointer) != base + 0x33363a0) return;
    auto* ratios = reinterpret_cast<float*>(state + 0x114);
    auto* lock = reinterpret_cast<uint32_t*>(state + 0x128);
    const auto width = *reinterpret_cast<uint32_t*>(state + 0x24), height = *reinterpret_cast<uint32_t*>(state + 0x28);
    rsf_game_render_config config{sizeof(config)};
    if (renderer_thread() == GetCurrentThreadId() && host.process_upscale && host.render_config && width && height && width <= 16384 && height <= 16384) {
        uintptr_t window{}; ID3D11DeviceContext* native_context{}; IDXGISwapChain* native_chain{};
        if (read(window, base + 0x3330198) && window && read(native_context, base + 0x3331f30) && native_context &&
            read(native_chain, window + 0x18) && native_chain) {
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
                "Skyrim native resolution plan %ux%u -> %ux%u ratios=%g,%g phases=%u saved_lock=%u; native auxiliary handoff required for reduced SR",
                render_width, render_height, width, height, double(ratios[0]), double(ratios[1]), phases, saved_lock);
            host.log(host.user, message);
        }
    }
}
}
