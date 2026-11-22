// SPDX-License-Identifier: MIT
// Typed native render continuations and foreign exception guards.
#include <rescaleframe/game_api.h>
#include <windows.h>
#include "cpu_call.h"
#include "observer_platform.h"
#include "postprocess_platform.h"
#include "render_observer_platform.h"
#include "session_platform.h"
#include <rescaleframe/d3d11_host.h>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

extern "C" bool rsf_skyrim_resolution_controls(uintptr_t) noexcept;
extern "C" void rsf_skyrim_observer_enter() noexcept;
extern "C" void rsf_skyrim_observer_leave() noexcept;

extern "C" bool rsf_skyrim_log_guarded(const rsf_game_host_services* host, const char* message) noexcept try {
    host->log(host->user, message);
    return true;
} catch (...) { return false; }

extern "C" bool rsf_skyrim_render_pass_guarded(const rsf_game_host_services* host, void* context,
    const rsf_game_render_pass* pass, uint32_t begin) noexcept try {
    host->render_pass(host->user, context, pass, begin);
    return true;
} catch (...) { return false; }

extern "C" int32_t rsf_skyrim_upscale_guarded(const rsf_game_host_services* host, void* context,
    const rsf_game_render_pass* pass) noexcept try {
    return host->process_upscale(host->user, context, pass);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" bool rsf_skyrim_render_config_guarded(const rsf_game_host_services* host,
    rsf_game_render_config* config) noexcept try {
    (void)host->render_config(host->user, config);
    return true;
} catch (...) { return false; }

namespace {
// Keep CS's extra fourth argument and its fifth stack byte in the native signature.
using ProcessMain = uintptr_t (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
using UpdateJitter = void (*)(void*);
using RendererEnd = uintptr_t (*)();
using UploadFrame = void (*)(void*, uint8_t);
struct ObserverLease {
    ObserverLease() noexcept { rsf_skyrim_observer_enter(); }
    ~ObserverLease() { rsf_skyrim_observer_leave(); }
    ObserverLease(const ObserverLease&) = delete;
    ObserverLease& operator=(const ObserverLease&) = delete;
};

template<class Function> Function continuation(rsf_skyrim_observer_hook index) noexcept {
    return reinterpret_cast<Function>(rsf_skyrim_observer_original(index));
}

} // namespace

extern "C" uintptr_t rsf_skyrim_observe_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5) noexcept
{
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    ObserverLease activity;
    struct Restore {
        rsf_skyrim_taa_receipt receipt{};
        ~Restore() { rsf_skyrim_postprocess_restore(&receipt); }
    } restore;
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    rsf_skyrim_postprocess_prepare(&call, &restore.receipt);
    return continuation<ProcessMain>(RSF_SKYRIM_OBSERVER_PROCESS)(manager, input, output, argument4, argument5);
}

extern "C" void rsf_skyrim_observe_jitter(void* state) noexcept
{
    ObserverLease activity;
    continuation<UpdateJitter>(RSF_SKYRIM_OBSERVER_JITTER)(state);
    rsf_skyrim_render_jitter_observed(uintptr_t(state));
}

extern "C" void rsf_skyrim_observe_resolution(void* state) noexcept {
    ObserverLease activity;
    if (!rsf_skyrim_resolution_controls(uintptr_t(state))) continuation<UpdateJitter>(RSF_SKYRIM_OBSERVER_RESOLUTION)(state);
}

extern "C" uintptr_t rsf_skyrim_observe_end() noexcept
{
    ObserverLease activity;
    rsf_game_host_services host{};
    rsf_skyrim_render_end_enter(&host);
    const auto result = continuation<RendererEnd>(RSF_SKYRIM_OBSERVER_END)();
    rsf_skyrim_render_end_leave(&host);
    return result;
}

extern "C" void rsf_skyrim_observe_upload(void* renderer, uint8_t flags) noexcept
{
    ObserverLease activity;
    rsf_skyrim_upload_observation observation{};
    const bool uploading = rsf_skyrim_render_upload_enter(uintptr_t(renderer), &observation);
    continuation<UploadFrame>(RSF_SKYRIM_OBSERVER_UPLOAD)(renderer, flags);
    if (uploading) rsf_skyrim_render_upload_leave(&observation, flags);
}

extern "C" bool rsf_skyrim_host_create_guarded(uintptr_t address, const rsf_d3d11_host_setup* setup,
    rsf_game_host_services* services, int32_t* result) noexcept try
{
    *result = reinterpret_cast<decltype(&rsf_d3d11_host_create)>(address)(setup, services);
    return true;
} catch (...) { return false; }

extern "C" bool rsf_skyrim_host_active_guarded(uintptr_t address, void* user, uint32_t active,
    int32_t* result) noexcept try
{
    *result = reinterpret_cast<decltype(&rsf_d3d11_host_set_active)>(address)(user, active);
    return true;
} catch (...) { return false; }
