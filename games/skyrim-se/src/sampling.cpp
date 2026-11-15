// SPDX-License-Identifier: GPL-3.0-only
#include "sampling.h"
#include "resolution.h"
#include "render_inputs.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>

namespace rsf::skyrim {
namespace {
using Flush = uintptr_t (*)(uint8_t);
Flush original{};
using Bind = decltype(&rsf_d3d11_host_bind_sampler_bias);
Bind bind{};
rsf_game_host_services host{};
std::atomic<bool> live{}, pending{};
std::atomic<unsigned> calls{};
void* target() { return reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x1010de0; }
void apply(bool restore) {
    if (!bind || !host.user || renderer_thread() != GetCurrentThreadId()) return;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const auto width = *reinterpret_cast<const uint32_t*>(base + 0x33363c4);
    const auto height = *reinterpret_cast<const uint32_t*>(base + 0x33363c8);
    uint32_t rw{}, rh{};
    // Native cached primary colour1/depth0 select the actual scene raster.
    // Postprocessing and UI switch colour targets; shadow views switch depth.
    const bool scene = !restore && live && resolution_extent(width, height, rw, rh) && rw < width &&
        *reinterpret_cast<const int32_t*>(base + 0x20cfc18) == 1 &&
        *reinterpret_cast<const int32_t*>(base + 0x20cfc38) == 0 &&
        *reinterpret_cast<const uint32_t*>(base + 0x33364c8) == 0 &&
        *reinterpret_cast<const float*>(base + 0x33364b4) < 1;
    if (!scene && !pending.load(std::memory_order_acquire)) return;
    rsf_d3d11_sampler_bias request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION, host.session_id};
    for (unsigned slot = 0; slot < 16; ++slot) {
        const auto address = reinterpret_cast<const uint32_t*>(base + 0x20cfcbc)[slot];
        const auto filter = reinterpret_cast<const uint32_t*>(base + 0x20cfcfc)[slot];
        if (address >= 6 || filter >= 5) return;
        request.samplers[slot] = *reinterpret_cast<void**>(base + 0x3331990 + 40 * address + 8 * filter);
        // Native filters2/3 are mip-linear and anisotropic. Filters0/1 clamp
        // MaxLOD to zero; filter4 is a comparison sampler used for shadows.
        if (filter == 2 || filter == 3) request.slot_mask |= 1u << slot;
    }
    if (scene) request.bias = std::log2(float(rw) / float(width));
    else request.slot_mask = 0;
    if (!request.slot_mask && !pending.load(std::memory_order_acquire)) return;
    auto* context = *reinterpret_cast<void**>(base + 0x3331f30);
    if (bind(host.user, context, &request) != RSF_OK) return;
    pending.store(request.applied_mask != 0, std::memory_order_release);
    static unsigned reports{};
    if (request.applied_mask && reports++ < 4 && host.log) {
        char message[180]{};
        std::snprintf(message, sizeof(message), "Skyrim scene sampler mip bias=%g slots=0x%x render=%ux%u output=%ux%u; native comparison/no-mip samplers retained",
            double(request.bias), request.applied_mask, rw, rh, width, height);
        host.log(host.user, message);
    }
}
uintptr_t observe(uint8_t preserve_input_layout) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    const auto result = original(preserve_input_layout);
    try { apply(false); } catch (...) {}
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}
}
bool sampling_guard() {
    constexpr unsigned char expected[]{0x4c,0x8b,0xdc,0x41,0x56,0x48,0x81,0xec,0xa0,0,0,0,0x8b,0x15,0x0e,0xee,0x0b,0x01};
    return std::memcmp(target(), expected, sizeof(expected)) == 0;
}
bool sampling_start(const rsf_game_host_services& services) {
    if (!original && (!sampling_guard() || MH_CreateHook(target(), reinterpret_cast<void*>(&observe), reinterpret_cast<void**>(&original)) != MH_OK)) return false;
    if (!live.load(std::memory_order_acquire) && !sampling_guard()) return false;
    bind = reinterpret_cast<Bind>(reinterpret_cast<void*>(GetProcAddress(GetModuleHandleW(L"ReScaleFrame.Runtime.dll"), "rsf_d3d11_host_bind_sampler_bias")));
    if (!bind) return false;
    host = services;
    const auto result = MH_EnableHook(target());
    if (result != MH_OK && result != MH_ERROR_ENABLED) return false;
    live.store(true, std::memory_order_release); return true;
}
void sampling_quiesce() { live.store(false, std::memory_order_release); }
bool sampling_busy() { return pending.load(std::memory_order_acquire) || calls.load(std::memory_order_acquire); }
bool sampling_stop() {
    sampling_quiesce();
    if (sampling_busy()) return false;
    if (!original) return true;
    const auto result = MH_DisableHook(target());
    return result == MH_OK || result == MH_ERROR_DISABLED;
}
void finish_sampling() { if (pending.load(std::memory_order_acquire)) apply(true); }
}
