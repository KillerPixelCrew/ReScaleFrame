// SPDX-License-Identifier: MIT
#include "sampling.h"
#include "native_creation_platform.h"
#include "observer_platform.h"
#include "render_inputs.h"
#include "resolution.h"
#include <windows.h>
#include <cstdio>

extern "C" {
struct rsf_skyrim_sampling_leaves {
    bool (*on_renderer_thread)();
    bool (*extent)(uint32_t, uint32_t, uint32_t*, uint32_t*);
    int32_t (*bind)(uintptr_t, void*, void*, rsf_d3d11_sampler_bias*);
    void (*report)(const rsf_game_host_services*, float, uint32_t, uint32_t, uint32_t, uint32_t, uint32_t);
};
bool rsf_skyrim_sampling_guard(const rsf_skyrim_creation_platform*) noexcept;
bool rsf_skyrim_sampling_start(const rsf_game_host_services*, const rsf_skyrim_creation_platform*,
    const rsf_skyrim_sampling_leaves*, const void* detour) noexcept;
void rsf_skyrim_sampling_quiesce() noexcept;
bool rsf_skyrim_sampling_busy() noexcept;
bool rsf_skyrim_sampling_stop(const rsf_skyrim_creation_platform*) noexcept;
void* rsf_skyrim_sampling_enter() noexcept;
void rsf_skyrim_sampling_leave(bool returned) noexcept;
}

static_assert(sizeof(rsf_skyrim_sampling_leaves) == 32);
static_assert(sizeof(rsf_d3d11_sampler_bias) == 160 && offsetof(rsf_d3d11_sampler_bias, slot_mask) == 144 &&
    offsetof(rsf_d3d11_sampler_bias, applied_mask) == 152);
static_assert(RSF_OK == 0);

namespace rsf::skyrim {
namespace {
using Flush = uintptr_t (*)(uint8_t);
using Bind = decltype(&rsf_d3d11_host_bind_sampler_bias);

uintptr_t observe(uint8_t preserve_input_layout) noexcept
{
    const auto original = reinterpret_cast<Flush>(rsf_skyrim_sampling_enter());
    struct Call {
        bool returned{};
        ~Call() { rsf_skyrim_sampling_leave(returned); }
    } call;
    if (!original) return 0;
    const auto result = original(preserve_input_layout);
    call.returned = true;
    return result;
}

constexpr rsf_skyrim_sampling_leaves leaves{
    []() noexcept { return renderer_thread() == GetCurrentThreadId(); },
    [](uint32_t w, uint32_t h, uint32_t* rw, uint32_t* rh) noexcept { return resolution_extent(w, h, *rw, *rh); },
    [](uintptr_t service, void* user, void* context, rsf_d3d11_sampler_bias* request) noexcept -> int32_t {
        try { return reinterpret_cast<Bind>(service)(user, context, request); }
        catch (...) { return RSF_ERROR_NOT_READY; }
    },
    [](const rsf_game_host_services* host, float bias, uint32_t mask, uint32_t rw, uint32_t rh,
        uint32_t width, uint32_t height) noexcept {
        if (host->log) try {
            char message[180]{};
            std::snprintf(message, sizeof(message), "Skyrim scene sampler mip bias=%g slots=0x%x render=%ux%u output=%ux%u; native comparison/no-mip samplers retained",
                double(bias), mask, rw, rh, width, height);
            host->log(host->user, message);
        } catch (...) {}
    },
};
}

bool sampling_guard() { return rsf_skyrim_sampling_guard(creation_platform()); }
bool sampling_start(const rsf_game_host_services& services)
{
    return rsf_skyrim_sampling_start(&services, creation_platform(), &leaves, reinterpret_cast<const void*>(&observe));
}
void sampling_quiesce() { rsf_skyrim_sampling_quiesce(); }
bool sampling_busy() { return rsf_skyrim_sampling_busy(); }
bool sampling_stop() { return rsf_skyrim_sampling_stop(creation_platform()); }
}
