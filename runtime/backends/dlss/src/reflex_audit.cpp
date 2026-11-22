// SPDX-License-Identifier: MIT
#include "reflex_audit.h"
#if RSF_HAVE_NVAPI_DIAGNOSTICS
#include <d3d12.h>
#include <dxgi.h>
#include <nvapi.h>
#include <MinHook.h>
#include <cstdio>
#include <cwchar>
#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_AUDIT_CALLER _ReturnAddress()
#else
#define RSF_AUDIT_CALLER __builtin_return_address(0)
#endif
using RsfReflexAuditEmit = void (*)(rsf_backend_log_fn, void*, const void*);
struct rsf_reflex_sync_report {
    uintptr_t caller, device;
    int32_t queue_us;
    uint32_t target_us, vblank_us, fg, dfg_max, dfg_fps;
    int32_t result;
    uint32_t enabled;
};
static_assert(sizeof(rsf_reflex_sync_report) == 48 && NVAPI_OK == 0);

struct rsf_reflex_audit_hook {
    void* target;
    void* detour;
    void** original;
};
struct rsf_reflex_audit_platform {
    uint32_t (*discover)(rsf_reflex_audit_hook*);
    uint32_t (*initialize)();
    uint32_t (*create)(const rsf_reflex_audit_hook*);
    uint32_t (*pin)(void*);
    uint32_t (*enable)(void*);
    void (*disable)(void*);
    void (*remove)(void*);
};
static_assert(sizeof(rsf_reflex_audit_hook) == 3 * sizeof(void*));
static_assert(sizeof(rsf_reflex_audit_platform) == 7 * sizeof(void*));

extern "C" {
void rsf_core_reflex_audit_start(rsf_backend_log_fn, void*, const rsf_reflex_audit_platform*, RsfReflexAuditEmit);
void rsf_core_reflex_audit_stop();
void rsf_core_reflex_audit_event(uintptr_t, uintptr_t, uint32_t, uint64_t, const void*, RsfReflexAuditEmit);
void rsf_core_reflex_audit_sync(const rsf_reflex_sync_report*, uint32_t, uint32_t);
void rsf_core_reflex_audit_report(RsfReflexAuditEmit);
}

namespace {
decltype(&NvAPI_D3D_SetSleepMode) original_mode = nullptr;
decltype(&NvAPI_D3D_SetLatencyMarker) original_marker = nullptr;
decltype(&NvAPI_D3D_SetReflexSync) original_sync = nullptr;
decltype(&NvAPI_D3D12_SetAsyncFrameMarker) original_async = nullptr;
struct Origin {
    wchar_t name[MAX_PATH];
    uintptr_t offset;
};

Origin provenance(void* caller)
{
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(caller), &module);
    wchar_t path[MAX_PATH]{};
    if (module) GetModuleFileNameW(module, path, MAX_PATH);
    const auto* leaf = std::wcsrchr(path, L'\\');
    Origin origin{};
    std::swprintf(origin.name, MAX_PATH, L"%ls", leaf ? leaf + 1 : path[0] ? path : L"unknown");
    origin.offset = uintptr_t(caller) - uintptr_t(module);
    return origin;
}
template<class Object, class Params>
struct Event {
    void* caller;
    Object* object;
    const Params* params;
    int result;
};
using ModeEvent = Event<IUnknown, NV_SET_SLEEP_MODE_PARAMS>;
using MarkerEvent = Event<IUnknown, NV_LATENCY_MARKER_PARAMS>;
using AsyncEvent = Event<ID3D12CommandQueue, NV_ASYNC_FRAME_MARKER_PARAMS>;

void emit_text(rsf_backend_log_fn log, void* user, const void* text) noexcept try
{
    log(user, static_cast<const char*>(text));
} catch (...) {}

void emit_mode(rsf_backend_log_fn log, void* user, const void* payload) noexcept try
{
    const auto& event = *static_cast<const ModeEvent*>(payload);
    const auto* params = event.params;
    const auto origin = provenance(event.caller);
    char text[480];
    std::snprintf(text, sizeof(text),
        "Reflex API mode caller=%ls+0x%llx device=%p low_latency=%u boost=%u minimum_us=%u marker_opt=%u min_queue=%u result=%d thread=%lu",
        origin.name, static_cast<unsigned long long>(origin.offset), static_cast<void*>(event.object), unsigned(params->bLowLatencyMode),
        unsigned(params->bLowLatencyBoost), params->minimumIntervalUs, unsigned(params->bUseMarkersToOptimize),
        unsigned(params->bUseMinQueueTime), event.result, GetCurrentThreadId());
    log(user, text);
} catch (...) {}

void emit_marker(rsf_backend_log_fn log, void* user, const void* payload) noexcept try
{
    const auto& event = *static_cast<const MarkerEvent*>(payload);
    const auto* params = event.params;
    const auto origin = provenance(event.caller);
    char text[480];
    std::snprintf(text, sizeof(text),
        "Reflex API marker source caller=%ls+0x%llx device=%p marker=%u frame=%llu high_bits=%llu result=%d thread=%lu",
        origin.name, static_cast<unsigned long long>(origin.offset), static_cast<void*>(event.object), unsigned(params->markerType),
        static_cast<unsigned long long>(params->frameID), static_cast<unsigned long long>(params->frameID >> 32),
        event.result, GetCurrentThreadId());
    log(user, text);
} catch (...) {}

void emit_async(rsf_backend_log_fn log, void* user, const void* payload) noexcept try
{
    const auto& event = *static_cast<const AsyncEvent*>(payload);
    const auto* params = event.params;
    const auto origin = provenance(event.caller);
    char text[480];
    std::snprintf(text, sizeof(text),
        "Reflex API async source caller=%ls+0x%llx queue=%p marker=%u frame=%llu present_frame=%llu vendor=%u result=%d thread=%lu",
        origin.name, static_cast<unsigned long long>(origin.offset), static_cast<void*>(event.object), unsigned(params->markerType),
        static_cast<unsigned long long>(params->frameID), static_cast<unsigned long long>(params->presentFrameID),
        unsigned(params->vendorInternal), event.result, GetCurrentThreadId());
    log(user, text);
} catch (...) {}

void emit_sync(rsf_backend_log_fn log, void* user, const void* payload) noexcept try
{
    const auto& report = *static_cast<const rsf_reflex_sync_report*>(payload);
    const auto origin = provenance(reinterpret_cast<void*>(report.caller));
    char text[480];
    std::snprintf(text, sizeof(text),
        "Reflex API sync caller=%ls+0x%llx device=%p enabled=%s queue_us=%d target_us=%u vblank_us=%u fg=%u dfg_max=%u dfg_fps=%u result=%d",
        origin.name, static_cast<unsigned long long>(origin.offset), reinterpret_cast<void*>(report.device),
        report.enabled == 1 ? "1" : report.enabled == 0 ? "0" : "unknown",
        report.queue_us, report.target_us, report.vblank_us, report.fg, report.dfg_max, report.dfg_fps, report.result);
    log(user, text);
} catch (...) {}

NvAPI_Status __cdecl observe_mode(IUnknown* device, NV_SET_SLEEP_MODE_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_mode(device, params);
    if (!params || params->version != NV_SET_SLEEP_MODE_PARAMS_VER) return result;
    const uint64_t settings = uint64_t(params->minimumIntervalUs) << 32 |
        uint32_t(params->bLowLatencyMode) | uint32_t(params->bLowLatencyBoost) << 1 |
        uint32_t(params->bUseMarkersToOptimize) << 2 | uint32_t(params->bUseMinQueueTime) << 3;
    const ModeEvent event{caller, device, params, int(result)};
    rsf_core_reflex_audit_event(uintptr_t(caller), uintptr_t(device), 0, settings, &event, emit_mode);
    return result;
}

NvAPI_Status __cdecl observe_marker(IUnknown* device, NV_LATENCY_MARKER_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_marker(device, params);
    if (!params || params->version != NV_LATENCY_MARKER_PARAMS_VER) return result;
    const MarkerEvent event{caller, device, params, int(result)};
    rsf_core_reflex_audit_event(uintptr_t(caller), uintptr_t(device), 1u + uint32_t(params->markerType),
        params->frameID >> 32, &event, emit_marker);
    return result;
}

NvAPI_Status __cdecl observe_sync(IUnknown* device, NV_SET_REFLEX_SYNC_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_sync(device, params);
    if (params && params->version == NV_SET_REFLEX_SYNC_PARAMS_VER) {
        const rsf_reflex_sync_report report{uintptr_t(caller), uintptr_t(device), params->timeInQueueUs,
            params->timeInQueueUsTarget, params->vblankIntervalUs, unsigned(params->fgMultiplier),
            unsigned(params->dfgMaxMultiplier), params->dfgTargetFps, int(result), 0};
        rsf_core_reflex_audit_sync(&report, uint32_t(params->bEnable), uint32_t(params->bDisable));
    }
    return result;
}

NvAPI_Status __cdecl observe_async(ID3D12CommandQueue* queue, NV_ASYNC_FRAME_MARKER_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_async(queue, params);
    if (!params || params->version != NV_ASYNC_FRAME_MARKER_PARAMS_VER) return result;
    const AsyncEvent event{caller, queue, params, int(result)};
    rsf_core_reflex_audit_event(uintptr_t(caller), uintptr_t(queue), 100u + uint32_t(params->markerType),
        uint64_t(params->vendorInternal), &event, emit_async);
    return result;
}

uint32_t discover(rsf_reflex_audit_hook* hooks)
{
    const auto query = rsf_nvapi_query();
    if (!query) return 0;
    hooks[0] = {query(0xac1ca9e0u), reinterpret_cast<void*>(&observe_mode), reinterpret_cast<void**>(&original_mode)};
    hooks[1] = {query(0xd9984c05u), reinterpret_cast<void*>(&observe_marker), reinterpret_cast<void**>(&original_marker)};
    hooks[2] = {query(0xb9f6faffu), reinterpret_cast<void*>(&observe_sync), reinterpret_cast<void**>(&original_sync)};
    hooks[3] = {query(0x13c98f73u), reinterpret_cast<void*>(&observe_async), reinterpret_cast<void**>(&original_async)};
    return 1;
}

constexpr rsf_reflex_audit_platform platform{
    discover,
    []() -> uint32_t {
        const auto result = MH_Initialize();
        return result == MH_OK || result == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](const rsf_reflex_audit_hook* hook) -> uint32_t {
        return MH_CreateHook(hook->target, hook->detour, hook->original) == MH_OK;
    },
    [](void* address) -> uint32_t {
        HMODULE module = nullptr;
        return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            reinterpret_cast<LPCWSTR>(address), &module) != 0;
    },
    [](void* target) -> uint32_t { return MH_EnableHook(target) == MH_OK; },
    [](void* target) { MH_DisableHook(target); },
    [](void* target) { MH_RemoveHook(target); },
};
} // namespace

RsfNvapiQuery rsf_nvapi_query()
{
    const auto module = GetModuleHandleW(L"nvapi64.dll");
    return module ? reinterpret_cast<RsfNvapiQuery>(GetProcAddress(module, "nvapi_QueryInterface")) : nullptr;
}
void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user)
{
    rsf_core_reflex_audit_start(log, user, &platform, emit_text);
}
void rsf_reflex_audit_stop() { rsf_core_reflex_audit_stop(); }
void rsf_reflex_audit_report() { rsf_core_reflex_audit_report(emit_sync); }
#else
RsfNvapiQuery rsf_nvapi_query() { return nullptr; }
void rsf_reflex_audit_start(rsf_backend_log_fn, void*) {}
void rsf_reflex_audit_stop() {}
void rsf_reflex_audit_report() {}
#endif
