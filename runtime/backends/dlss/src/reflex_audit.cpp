// SPDX-License-Identifier: GPL-3.0-only
// Optional bounded NVAPI observation. Each detour calls its original first and returns that
// exact result; counters identify caller/device/policy changes without retaining device ownership.
#include "reflex_audit.h"
#if RSF_HAVE_NVAPI_DIAGNOSTICS
#include <d3d12.h>
#include <dxgi.h>
#include <nvapi.h>
#include <MinHook.h>
#include <array>
#include <mutex>
#include <cstdio>
#include <cwchar>
#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_AUDIT_CALLER _ReturnAddress()
#else
#define RSF_AUDIT_CALLER __builtin_return_address(0)
#endif
namespace {
std::mutex guard;
rsf_backend_log_fn listener = nullptr;
void* listener_user = nullptr;
decltype(&NvAPI_D3D_SetSleepMode) original_mode = nullptr;
decltype(&NvAPI_D3D_SetLatencyMarker) original_marker = nullptr;
decltype(&NvAPI_D3D_SetReflexSync) original_sync = nullptr;
decltype(&NvAPI_D3D12_SetAsyncFrameMarker) original_async = nullptr;
bool installed = false;
/* Device/caller pointers are diagnostic identities only; never dereferenced or AddRef'd. */
struct Entry {
    void* caller = nullptr;
    IUnknown* device = nullptr;
    uint32_t kind = 0;
    uint64_t settings = 0;
};
std::array<Entry, 64> sources{};
uint32_t lines = 0;
struct SyncState {
    void* caller = nullptr;
    IUnknown* device = nullptr;
    NV_SET_REFLEX_SYNC_PARAMS params{};
    bool enabled = false;
    int result = 0;
} sync_state;
/* Remember up to 64 source/kind pairs and log only changed settings while the budget permits. */
bool changed(const Entry& next)
{
    for (auto& entry : sources) if (entry.caller == next.caller && entry.device == next.device && entry.kind == next.kind) {
        if (entry.settings == next.settings) return false;
        entry = next; return true;
    }
    for (auto& entry : sources) if (!entry.caller) { entry = next; return true; }
    return false;
}
/* Resolve caller address to module leaf and RVA without taking a module reference. */
void provenance(void* caller, wchar_t (&name)[MAX_PATH], uintptr_t& offset)
{
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(caller), &module);
    wchar_t path[MAX_PATH]{};
    if (module) GetModuleFileNameW(module, path, MAX_PATH);
    const auto* leaf = std::wcsrchr(path, L'\\');
    std::swprintf(name, MAX_PATH, L"%ls", leaf ? leaf + 1 : path[0] ? path : L"unknown");
    offset = uintptr_t(caller) - uintptr_t(module);
}
/* Record validated mode flags and the minimum interval in microseconds. */
NvAPI_Status __cdecl observe_mode(IUnknown* device, NV_SET_SLEEP_MODE_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_mode(device, params);
    if (!params || params->version != NV_SET_SLEEP_MODE_PARAMS_VER) return result;
    const uint64_t settings = uint64_t(params->minimumIntervalUs) << 32 |
        uint32_t(params->bLowLatencyMode) | uint32_t(params->bLowLatencyBoost) << 1 |
        uint32_t(params->bUseMarkersToOptimize) << 2 | uint32_t(params->bUseMinQueueTime) << 3;
    std::lock_guard<std::mutex> lock(guard);
    if (listener && lines < 192 && changed({caller, device, 0, settings})) {
        ++lines; wchar_t name[MAX_PATH]{}; uintptr_t offset = 0; provenance(caller, name, offset);
        char text[480]; std::snprintf(text, sizeof(text),
            "Reflex API mode caller=%ls+0x%llx device=%p low_latency=%u boost=%u minimum_us=%u marker_opt=%u min_queue=%u result=%d thread=%lu",
            name, static_cast<unsigned long long>(offset), static_cast<void*>(device), unsigned(params->bLowLatencyMode),
            unsigned(params->bLowLatencyBoost), params->minimumIntervalUs, unsigned(params->bUseMarkersToOptimize),
            unsigned(params->bUseMinQueueTime), int(result), GetCurrentThreadId());
        listener(listener_user, text);
    }
    return result;
}
/* Compare upper frame-ID bits so ordinary per-frame marker traffic stays bounded. */
NvAPI_Status __cdecl observe_marker(IUnknown* device, NV_LATENCY_MARKER_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_marker(device, params);
    if (!params || params->version != NV_LATENCY_MARKER_PARAMS_VER) return result;
    const uint32_t kind = 1u + uint32_t(params->markerType);
    const uint64_t wide = params->frameID >> 32;
    std::lock_guard<std::mutex> lock(guard);
    if (listener && lines < 192 && changed({caller, device, kind, wide})) {
        ++lines; wchar_t name[MAX_PATH]{}; uintptr_t offset = 0; provenance(caller, name, offset);
        char text[480]; std::snprintf(text, sizeof(text),
            "Reflex API marker source caller=%ls+0x%llx device=%p marker=%u frame=%llu high_bits=%llu result=%d thread=%lu",
            name, static_cast<unsigned long long>(offset), static_cast<void*>(device), unsigned(params->markerType),
            static_cast<unsigned long long>(params->frameID), static_cast<unsigned long long>(wide), int(result), GetCurrentThreadId());
        listener(listener_user, text);
    }
    return result;
}
/* Cache the most recent sync request; enabled state changes only after successful SDK results. */
NvAPI_Status __cdecl observe_sync(IUnknown* device, NV_SET_REFLEX_SYNC_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_sync(device, params);
    if (params && params->version == NV_SET_REFLEX_SYNC_PARAMS_VER) {
        std::lock_guard<std::mutex> lock(guard);
        sync_state.caller = caller; sync_state.device = device; sync_state.params = *params; sync_state.result = int(result);
        if (result == NVAPI_OK) {
            if (params->bEnable) sync_state.enabled = true;
            if (params->bDisable) sync_state.enabled = false;
        }
    }
    return result;
}
/* Record async marker provenance/vendor changes while preserving the original call's result. */
NvAPI_Status __cdecl observe_async(ID3D12CommandQueue* queue, NV_ASYNC_FRAME_MARKER_PARAMS* params)
{
    void* caller = RSF_AUDIT_CALLER;
    const auto result = original_async(queue, params);
    if (!params || params->version != NV_ASYNC_FRAME_MARKER_PARAMS_VER) return result;
    std::lock_guard<std::mutex> lock(guard);
    if (listener && lines < 192 && changed({caller, queue, 100u + uint32_t(params->markerType), uint64_t(params->vendorInternal)})) {
        ++lines; wchar_t name[MAX_PATH]{}; uintptr_t offset = 0; provenance(caller, name, offset);
        char text[480]; std::snprintf(text, sizeof(text),
            "Reflex API async source caller=%ls+0x%llx queue=%p marker=%u frame=%llu present_frame=%llu vendor=%u result=%d thread=%lu",
            name, static_cast<unsigned long long>(offset), static_cast<void*>(queue), unsigned(params->markerType),
            static_cast<unsigned long long>(params->frameID), static_cast<unsigned long long>(params->presentFrameID),
            unsigned(params->vendorInternal), int(result), GetCurrentThreadId());
        listener(listener_user, text);
    }
    return result;
}
}
/** Resolve hooks only from an already loaded NVAPI module. Install all four observers or remove
 * the partial installation; pin both code owners so future external hook chains remain callable.
 */
void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user)
{
    std::lock_guard<std::mutex> lock(guard);
    listener = log; listener_user = user; sources = {}; lines = 0; sync_state = {};
    if (installed) return;
    const auto module = GetModuleHandleW(L"nvapi64.dll");
    using Query = void*(__cdecl*)(unsigned int);
    auto query = module ? reinterpret_cast<Query>(GetProcAddress(module, "nvapi_QueryInterface")) : nullptr;
    if (!query) { if (listener) listener(listener_user, "Reflex API audit unavailable: NVAPI module/export absent"); return; }
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) return;
    void* targets[]{query(0xac1ca9e0u), query(0xd9984c05u), query(0xb9f6faffu), query(0x13c98f73u)};
    void* hooks[]{reinterpret_cast<void*>(&observe_mode), reinterpret_cast<void*>(&observe_marker),
        reinterpret_cast<void*>(&observe_sync), reinterpret_cast<void*>(&observe_async)};
    void** originals[]{reinterpret_cast<void**>(&original_mode), reinterpret_cast<void**>(&original_marker),
        reinterpret_cast<void**>(&original_sync), reinterpret_cast<void**>(&original_async)};
    uint32_t made = 0;
    for (; made < 4; ++made) if (!targets[made] || MH_CreateHook(targets[made], hooks[made], originals[made]) != MH_OK) break;
    bool enabled = made == 4;
    for (uint32_t i = 0; enabled && i < made; ++i) enabled = MH_EnableHook(targets[i]) == MH_OK;
    if (!enabled) {
        for (uint32_t i = 0; i < made; ++i) { MH_DisableHook(targets[i]); MH_RemoveHook(targets[i]); }
        if (listener) listener(listener_user, "Reflex API audit refused: existing entry cannot be chained safely");
        return;
    }
    installed = true;
    // External tools may attach after us and retain our entry as their original. Both
    // target and observer code must therefore remain resident until process exit.
    HMODULE pinned = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(targets[0]), &pinned);
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(&observe_mode), &pinned);
    if (listener) listener(listener_user, "Reflex API audit installed: observing mode, sync and marker callers without filtering");
}
void rsf_reflex_audit_stop()
{
    // Detours stay with the process-owned backend module, preserving later external chains.
    // Drain the bounded logging callback before its owner can be destroyed.
    std::lock_guard<std::mutex> lock(guard); listener = nullptr; listener_user = nullptr;
}
void rsf_reflex_audit_report()
{
    std::lock_guard<std::mutex> lock(guard);
    if (!listener || !sync_state.caller) return;
    wchar_t name[MAX_PATH]{}; uintptr_t offset = 0; provenance(sync_state.caller, name, offset);
    const auto& params = sync_state.params;
    char text[480]; std::snprintf(text, sizeof(text),
        "Reflex API sync caller=%ls+0x%llx device=%p enabled=%u queue_us=%d target_us=%u vblank_us=%u fg=%u dfg_max=%u dfg_fps=%u result=%d",
        name, static_cast<unsigned long long>(offset), static_cast<void*>(sync_state.device), unsigned(sync_state.enabled),
        params.timeInQueueUs, params.timeInQueueUsTarget, params.vblankIntervalUs, unsigned(params.fgMultiplier),
        unsigned(params.dfgMaxMultiplier), params.dfgTargetFps, sync_state.result);
    listener(listener_user, text);
}
#else
// Audit calls remain available as no-ops when optional NVAPI headers were absent at build time.
void rsf_reflex_audit_start(rsf_backend_log_fn, void*) {}
void rsf_reflex_audit_stop() {}
void rsf_reflex_audit_report() {}
#endif
