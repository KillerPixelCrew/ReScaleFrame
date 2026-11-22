// SPDX-License-Identifier: MIT
// GameSDK export, native detours and leaves of the Rust Skyrim SE owner (src/session.rs). Rust owns
// the lifecycle order, results and reasons, the Runtime host setup and its original-session reuse,
// DPI startup, the observer hook record and the borrowed services, which the detours lease under
// the session lock. This file keeps the native observer detours, return-address acquisition,
// and the exception boundary for callbacks supplied to Rust.
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <atomic>
#include "cpu_events.h"
#include "compatibility.h"
#include "postprocess_platform.h"
#include "render_observer_platform.h"
#include "session_platform.h"
#include <rescaleframe/d3d11_host.h>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

extern "C" bool rsf_skyrim_resolution_controls(uintptr_t) noexcept;

extern "C" bool rsf_skyrim_call_log(const rsf_game_host_services* host, const char* message) noexcept try {
    if (host && host->log) host->log(host->user, message);
    return true;
} catch (...) { return false; }

extern "C" bool rsf_skyrim_call_render_pass(const rsf_game_host_services* host, void* context,
    const rsf_game_render_pass* pass, uint32_t begin) noexcept try {
    if (!host->render_pass) return false;
    host->render_pass(host->user, context, pass, begin);
    return true;
} catch (...) { return false; }

extern "C" bool rsf_skyrim_call_upscale(const rsf_game_host_services* host, void* context,
    const rsf_game_render_pass* pass) noexcept try {
    return host->process_upscale && host->process_upscale(host->user, context, pass) == RSF_OK;
} catch (...) { return false; }

extern "C" void rsf_skyrim_call_render_config(const rsf_game_host_services* host,
    rsf_game_render_config* config) noexcept try {
    (void)host->render_config(host->user, config);
} catch (...) { *config = {sizeof(*config)}; }

namespace {
// Keep CS's extra fourth argument and its fifth stack byte in the native signature.
using ProcessMain = uintptr_t (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
using UpdateJitter = void (*)(void*);
using RendererEnd = uintptr_t (*)();
using UploadFrame = void (*)(void*, uint8_t);
std::atomic<uint32_t> calls{0};
using CreateHost = int32_t (*)(const rsf_d3d11_host_setup*, rsf_game_host_services*);
using SetHostActive = int32_t (*)(void*, uint32_t);

enum ObserverIndex : uint32_t { jitter, process, resolution, end, upload, observer_count };

template<class Function> Function continuation(ObserverIndex index) noexcept {
    return reinterpret_cast<Function>(rsf_skyrim_observer_original(index));
}

uintptr_t observe_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5) noexcept
{
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    calls.fetch_add(1, std::memory_order_acq_rel);
    struct Call {
        ~Call() { calls.fetch_sub(1, std::memory_order_acq_rel); }
    } activity;
    struct Restore {
        rsf_skyrim_taa_receipt receipt{};
        ~Restore() { rsf_skyrim_postprocess_restore(&receipt); }
    } restore;
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    rsf_skyrim_postprocess_prepare(&call, &restore.receipt);
    return continuation<ProcessMain>(process)(manager, input, output, argument4, argument5);
}

void observe_jitter(void* state) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    continuation<UpdateJitter>(jitter)(state);
    rsf_skyrim_render_jitter_observed(uintptr_t(state));
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

void observe_resolution(void* state) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    if (!rsf_skyrim_resolution_controls(uintptr_t(state))) continuation<UpdateJitter>(resolution)(state);
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

uintptr_t observe_end() noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    rsf_game_host_services host{};
    rsf_skyrim_render_end_enter(&host);
    const auto result = continuation<RendererEnd>(end)();
    rsf_skyrim_render_end_leave(&host);
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}

void observe_upload(void* renderer, uint8_t flags) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    rsf_skyrim_upload_observation observation{};
    const bool uploading = rsf_skyrim_render_upload_enter(uintptr_t(renderer), &observation);
    continuation<UploadFrame>(upload)(renderer, flags);
    if (uploading) rsf_skyrim_render_upload_leave(&observation, flags);
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

constexpr rsf_skyrim_observer_leaves observer_leaves{
    rsf_skyrim_observer_initialize,
    rsf_skyrim_observer_create,
    rsf_skyrim_observer_remove,
    rsf_skyrim_observer_enable,
    rsf_skyrim_observer_disable,
    rsf_skyrim_observer_pin,
    [](const rsf_game_host_services* host, const char* message) noexcept {
        (void)rsf_skyrim_call_log(host, message);
    },
};

const rsf_skyrim_session_leaves session_leaves{
    sizeof(rsf_skyrim_session_leaves), RSF_SKYRIM_SESSION_LEAVES_VERSION,
    rsf::skyrim::creation_platform(), &observer_leaves,
    rsf_skyrim_observer_admit,
    []() noexcept { return calls.load(std::memory_order_acquire) != 0; },
    rsf_skyrim_cpu_prepare,
    rsf_skyrim_cpu_start,
    [](const char** refusal, bool arm) noexcept {
        return rsf_skyrim_cs_checkpoint(refusal, arm ? &rsf_skyrim_session_withdraw : nullptr);
    },
    rsf_skyrim_cs_status,
    rsf_skyrim_call_log,
    [](uintptr_t create, const rsf_d3d11_host_setup* setup, rsf_game_host_services* services, int32_t* result) noexcept {
        try { *result = reinterpret_cast<CreateHost>(create)(setup, services); return true; }
        catch (...) { return false; }
    },
    [](uintptr_t activate, void* user, uint32_t active, int32_t* result) noexcept {
        try { *result = reinterpret_cast<SetHostActive>(activate)(user, active); return true; }
        catch (...) { return false; }
    },
    rsf_skyrim_render_accept_window,
};
}

extern "C" void* rsf_skyrim_observer_detour(uint32_t index) noexcept
{
    static void* const detours[observer_count]{
        reinterpret_cast<void*>(&observe_jitter),
        reinterpret_cast<void*>(&observe_process),
        reinterpret_cast<void*>(&observe_resolution),
        reinterpret_cast<void*>(&observe_end),
        reinterpret_cast<void*>(&observe_upload),
    };
    return index < observer_count ? detours[index] : nullptr;
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_skyrim_publish_api(requested_abi, api, &session_leaves);
}
