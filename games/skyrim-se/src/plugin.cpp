// SPDX-License-Identifier: MIT
// GameSDK export, native detours and leaves of the Rust Skyrim SE owner (src/session.rs). Rust owns
// the lifecycle order, results and reasons, the Runtime host setup and its original-session reuse,
// DPI startup, the observer hook record and the borrowed services, which the detours lease under
// the session lock. This file keeps the native observer detours, return-address acquisition,
// MinHook and the exception boundary for callbacks supplied to Rust.
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <MinHook.h>
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

extern "C" bool rsf_skyrim_presentation_guard() noexcept;
extern "C" bool rsf_skyrim_presentation_start(const rsf_game_host_services*) noexcept;
extern "C" void rsf_skyrim_presentation_quiesce() noexcept;
extern "C" bool rsf_skyrim_presentation_busy() noexcept;
extern "C" bool rsf_skyrim_presentation_stop() noexcept;
extern "C" bool rsf_skyrim_sampling_guard() noexcept;
extern "C" bool rsf_skyrim_sampling_start(const rsf_game_host_services*) noexcept;
extern "C" void rsf_skyrim_sampling_quiesce() noexcept;
extern "C" bool rsf_skyrim_sampling_busy() noexcept;
extern "C" bool rsf_skyrim_sampling_stop() noexcept;
extern "C" bool rsf_skyrim_resolution_owned() noexcept;
extern "C" bool rsf_skyrim_resolution_controls(uintptr_t) noexcept;

extern "C" bool rsf_skyrim_render_allowed() noexcept {
    return rsf::skyrim::compatibility_render_allowed();
}

extern "C" bool rsf_skyrim_matches_postprocess(const rsf_skyrim_cs_hdr_call* call) noexcept {
    return rsf::skyrim::compatibility_postprocess_call(call->caller, reinterpret_cast<void*>(call->manager),
        call->input, call->output, call->argument4, call->argument5);
}

extern "C" uint64_t rsf_skyrim_render_source() noexcept {
    return rsf::skyrim::cpu_source();
}

extern "C" bool rsf_skyrim_render_identity(rsf_game_render_pass* pass) noexcept try {
    return rsf::skyrim::cpu_render_identity(*pass);
} catch (...) { return false; }

extern "C" bool rsf_skyrim_render_eligible() noexcept {
    return rsf::skyrim::cpu_rendered_source_eligible();
}

extern "C" void rsf_skyrim_render_diagnostic(const char* stage, const rsf_game_render_pass* pass) noexcept try {
    rsf::skyrim::cpu_render_diagnostic(stage, *pass);
} catch (...) {}

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
using UpdateJitter = void (*)(void*);
UpdateJitter original = nullptr;
void* target = nullptr;
// The matched game consumes three arguments. Carry the CS wrapper's two extra
// machine arguments unchanged, including its fifth stack byte (not a bool read).
using ProcessMain = uintptr_t (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
ProcessMain original_process = nullptr;
void* process_target = nullptr;
UpdateJitter original_resolution = nullptr;
void* resolution_target = nullptr;
using RendererEnd = uintptr_t (*)();
RendererEnd original_end = nullptr;
void* end_target = nullptr;
using UploadFrame = void (*)(void*, uint8_t);
UploadFrame original_upload = nullptr;
void* upload_target = nullptr;
std::atomic<uint32_t> calls{0};
using CreateHost = int32_t (*)(const rsf_d3d11_host_setup*, rsf_game_host_services*);
using SetHostActive = int32_t (*)(void*, uint32_t);
int accept_presentation_window(void*, void* window) noexcept
{
    return rsf_skyrim_render_accept_window(window) ? 1 : 0;
}

bool accepting() noexcept { return rsf_skyrim_session_accepting(); }

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
    return original_process(manager, input, output, argument4, argument5);
}

void observe_jitter(void* state) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    original(state);
    rsf_skyrim_render_jitter_observed(uintptr_t(state));
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

void observe_resolution(void* state) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    if (!rsf_skyrim_resolution_controls(uintptr_t(state))) original_resolution(state);
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

uintptr_t observe_end() noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    rsf_game_host_services host{};
    rsf_skyrim_render_end_enter(&host);
    const auto result = original_end();
    rsf_skyrim_render_end_leave(&host);
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}

void observe_upload(void* renderer, uint8_t flags) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    rsf_skyrim_upload_observation observation{};
    const bool uploading = rsf_skyrim_render_upload_enter(uintptr_t(renderer), &observation);
    original_upload(renderer, flags);
    if (uploading) rsf_skyrim_render_upload_leave(&observation, flags);
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

int32_t admit_observers()
{
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(nullptr, path, 32768);
    rsf_skyrim_observer_targets sites{};
    const auto result = rsf_skyrim_observer_admit(path, length,
        []() noexcept -> int32_t { return rsf_skyrim_sampling_guard(); },
        []() noexcept -> int32_t { return rsf_skyrim_presentation_guard(); }, &sites);
    if (result == 1) {
        target = sites.jitter;
        process_target = sites.process;
        resolution_target = sites.resolution;
        end_target = sites.end;
        upload_target = sites.upload;
    }
    return result;
}

struct ObserverHook {
    void* const* address;
    void* detour;
    void** trampoline;
};
const ObserverHook& observer_hook(uint32_t index) noexcept
{
    static const ObserverHook hooks[]{
        {&target, reinterpret_cast<void*>(&observe_jitter), reinterpret_cast<void**>(&original)},
        {&process_target, reinterpret_cast<void*>(&observe_process), reinterpret_cast<void**>(&original_process)},
        {&resolution_target, reinterpret_cast<void*>(&observe_resolution), reinterpret_cast<void**>(&original_resolution)},
        {&end_target, reinterpret_cast<void*>(&observe_end), reinterpret_cast<void**>(&original_end)},
        {&upload_target, reinterpret_cast<void*>(&observe_upload), reinterpret_cast<void**>(&original_upload)},
    };
    return hooks[index];
}

constexpr rsf_skyrim_observer_leaves observer_leaves{
    []() noexcept {
        const auto result = MH_Initialize();
        return result == MH_OK || result == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](uint32_t index) noexcept {
        const auto& hook = observer_hook(index);
        return MH_CreateHook(*hook.address, hook.detour, hook.trampoline) == MH_OK;
    },
    [](uint32_t index) noexcept {
        const auto& hook = observer_hook(index);
        const auto result = MH_RemoveHook(*hook.address);
        if (result != MH_OK && result != MH_ERROR_NOT_CREATED) return false;
        *hook.trampoline = nullptr;
        return true;
    },
    [](uint32_t index) noexcept -> int32_t {
        return static_cast<int32_t>(MH_EnableHook(*observer_hook(index).address));
    },
    [](uint32_t index) noexcept {
        const auto result = MH_DisableHook(*observer_hook(index).address);
        return result == MH_OK || result == MH_ERROR_DISABLED;
    },
    []() noexcept {
        HMODULE pinned = nullptr;
        return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&observe_jitter), &pinned) != FALSE;
    },
    [](const rsf_game_host_services* host, const char* message) noexcept {
        if (host->log) try { host->log(host->user, message); } catch (...) {}
    },
};
static_assert(MH_OK == 0);

// A subsystem call that throws reports false; the Rust session then retires what it started and
// answers RSF_ERROR_NOT_READY as the former catch did.
bool subsystem(uint32_t operation, const rsf_game_host_services* services, int32_t* value) noexcept try
{
    using namespace rsf::skyrim;
    const auto flag = [value](bool result) { *value = result ? 1 : 0; };
    switch (operation) {
    case RSF_SKYRIM_ADMIT_OBSERVERS: *value = admit_observers(); break;
    case RSF_SKYRIM_OBSERVERS_BUSY: flag(calls.load(std::memory_order_acquire) != 0); break;
    case RSF_SKYRIM_RESET_OBSERVATIONS:
        rsf_skyrim_reset_render_observations();
        flag(true);
        break;
    case RSF_SKYRIM_CPU_PREPARE: flag(cpu_prepare()); break;
    case RSF_SKYRIM_CPU_BIND: flag(services && cpu_bind(*services)); break;
    case RSF_SKYRIM_CPU_START: flag(cpu_start()); break;
    case RSF_SKYRIM_CPU_QUIESCE: cpu_quiesce(); flag(true); break;
    case RSF_SKYRIM_CPU_DISABLE: flag(cpu_disable()); break;
    case RSF_SKYRIM_CPU_STOP: flag(cpu_stop()); break;
    case RSF_SKYRIM_CPU_BUSY: flag(cpu_busy()); break;
    case RSF_SKYRIM_SAMPLING_START: flag(services && rsf_skyrim_sampling_start(services)); break;
    case RSF_SKYRIM_SAMPLING_QUIESCE: rsf_skyrim_sampling_quiesce(); flag(true); break;
    case RSF_SKYRIM_SAMPLING_STOP: flag(rsf_skyrim_sampling_stop()); break;
    case RSF_SKYRIM_SAMPLING_BUSY: flag(rsf_skyrim_sampling_busy()); break;
    case RSF_SKYRIM_PRESENTATION_START: flag(services && rsf_skyrim_presentation_start(services)); break;
    case RSF_SKYRIM_PRESENTATION_QUIESCE: rsf_skyrim_presentation_quiesce(); flag(true); break;
    case RSF_SKYRIM_PRESENTATION_STOP: flag(rsf_skyrim_presentation_stop()); break;
    case RSF_SKYRIM_PRESENTATION_BUSY: flag(rsf_skyrim_presentation_busy()); break;
    case RSF_SKYRIM_RESOLUTION_OWNED: flag(rsf_skyrim_resolution_owned()); break;
    case RSF_SKYRIM_AUXILIARIES_PENDING: flag(rsf_skyrim_render_cleanup_pending()); break;
    default: flag(false); break;
    }
    return true;
}
catch (...) { return false; }

const rsf_skyrim_session_leaves session_leaves{
    sizeof(rsf_skyrim_session_leaves), RSF_SKYRIM_SESSION_LEAVES_VERSION,
    rsf::skyrim::creation_platform(), &observer_leaves,
    subsystem,
    [](const char** refusal, bool arm) noexcept {
        return rsf::skyrim::compatibility_checkpoint(*refusal, arm ? &rsf_skyrim_session_withdraw : nullptr);
    },
    []() noexcept { return rsf::skyrim::compatibility_status(); },
    [](const rsf_game_host_services* host, const char* message) noexcept {
        try { if (host && host->log) host->log(host->user, message); return true; }
        catch (...) { return false; }
    },
    [](uintptr_t module, wchar_t* path, uint32_t capacity) noexcept {
        return uint32_t(GetModuleFileNameW(reinterpret_cast<HMODULE>(module), path, capacity));
    },
    [](uintptr_t create, const rsf_d3d11_host_setup* setup, rsf_game_host_services* services, int32_t* result) noexcept {
        try { *result = reinterpret_cast<CreateHost>(create)(setup, services); return true; }
        catch (...) { return false; }
    },
    [](uintptr_t activate, void* user, uint32_t active, int32_t* result) noexcept {
        try { *result = reinterpret_cast<SetHostActive>(activate)(user, active); return true; }
        catch (...) { return false; }
    },
    accept_presentation_window,
};
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_skyrim_publish_api(requested_abi, api, &session_leaves);
}
