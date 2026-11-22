// SPDX-License-Identifier: MIT
#include "reflex_audit.h"
#include "streamline_diagnostics.h"
#if RSF_HAVE_NVAPI_DIAGNOSTICS
#include <d3d12.h>
#include <dxgi.h>
#include <nvapi.h>
#include <MinHook.h>
#include <cstddef>
#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_AUDIT_CALLER _ReturnAddress()
#else
#define RSF_AUDIT_CALLER __builtin_return_address(0)
#endif
static_assert(NV_GET_SLEEP_STATUS_PARAMS_VER == 0x10088 && NVAPI_NO_IMPLEMENTATION == -3);
static_assert(NV_SET_SLEEP_MODE_PARAMS_VER == 0x1002c);
static_assert(NV_LATENCY_MARKER_PARAMS_VER == 0x10058);
static_assert(NV_SET_REFLEX_SYNC_PARAMS_VER == 0x10030 && NVAPI_OK == 0);
static_assert(NV_ASYNC_FRAME_MARKER_PARAMS_VER == 0x10058);
static_assert(offsetof(NV_GET_SLEEP_STATUS_PARAMS, sleepIntervalUs) == 8 &&
    offsetof(NV_GET_SLEEP_STATUS_PARAMS, dfgFrameTimeTargetUs) == 16);
static_assert(offsetof(NV_SET_SLEEP_MODE_PARAMS, minimumIntervalUs) == 8 &&
    offsetof(NV_SET_SLEEP_MODE_PARAMS, bUseMinQueueTime) == 13);
static_assert(offsetof(NV_LATENCY_MARKER_PARAMS, frameID) == 8 &&
    offsetof(NV_LATENCY_MARKER_PARAMS, markerType) == 16);
static_assert(offsetof(NV_SET_REFLEX_SYNC_PARAMS, vblankIntervalUs) == 8 &&
    offsetof(NV_SET_REFLEX_SYNC_PARAMS, timeInQueueUs) == 12 &&
    offsetof(NV_SET_REFLEX_SYNC_PARAMS, dfgTargetFps) == 24);
static_assert(offsetof(NV_ASYNC_FRAME_MARKER_PARAMS, frameID) == 8 &&
    offsetof(NV_ASYNC_FRAME_MARKER_PARAMS, markerType) == 16 &&
    offsetof(NV_ASYNC_FRAME_MARKER_PARAMS, presentFrameID) == 24 &&
    offsetof(NV_ASYNC_FRAME_MARKER_PARAMS, vendorInternal) == 32);

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
FARPROC rsf_core_reflex_audit_query();
void rsf_core_reflex_audit_start(rsf_backend_log_fn, void*, const rsf_reflex_audit_platform*, RsfStreamlineEmitLog);
void rsf_core_reflex_audit_stop();
void rsf_core_reflex_nvapi_observe(uint32_t, uintptr_t, void*, const void*, int32_t, RsfStreamlineEmitLog);
void rsf_core_reflex_driver_sleep(void*, rsf_backend_log_fn, void*, RsfStreamlineEmitLog);
}

namespace {
decltype(&NvAPI_D3D_SetSleepMode) original_mode = nullptr;
decltype(&NvAPI_D3D_SetLatencyMarker) original_marker = nullptr;
decltype(&NvAPI_D3D_SetReflexSync) original_sync = nullptr;
decltype(&NvAPI_D3D12_SetAsyncFrameMarker) original_async = nullptr;
// Capture the actual native caller before invoking the original trampoline.
NvAPI_Status __cdecl observe_mode(IUnknown* device, NV_SET_SLEEP_MODE_PARAMS* params)
{
    const auto caller = uintptr_t(RSF_AUDIT_CALLER);
    const auto result = original_mode(device, params);
    rsf_core_reflex_nvapi_observe(0, caller, device, params, result, rsf::emit_streamline_log);
    return result;
}

NvAPI_Status __cdecl observe_marker(IUnknown* device, NV_LATENCY_MARKER_PARAMS* params)
{
    const auto caller = uintptr_t(RSF_AUDIT_CALLER);
    const auto result = original_marker(device, params);
    rsf_core_reflex_nvapi_observe(1, caller, device, params, result, rsf::emit_streamline_log);
    return result;
}

NvAPI_Status __cdecl observe_sync(IUnknown* device, NV_SET_REFLEX_SYNC_PARAMS* params)
{
    const auto caller = uintptr_t(RSF_AUDIT_CALLER);
    const auto result = original_sync(device, params);
    rsf_core_reflex_nvapi_observe(2, caller, device, params, result, rsf::emit_streamline_log);
    return result;
}

NvAPI_Status __cdecl observe_async(ID3D12CommandQueue* queue, NV_ASYNC_FRAME_MARKER_PARAMS* params)
{
    const auto caller = uintptr_t(RSF_AUDIT_CALLER);
    const auto result = original_async(queue, params);
    rsf_core_reflex_nvapi_observe(3, caller, queue, params, result, rsf::emit_streamline_log);
    return result;
}

uint32_t discover(rsf_reflex_audit_hook* hooks)
{
    using Query = void*(__cdecl*)(unsigned int);
    const auto query = reinterpret_cast<Query>(rsf_core_reflex_audit_query());
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

void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user)
{
    rsf_core_reflex_audit_start(log, user, &platform, rsf::emit_streamline_log);
}
void rsf_reflex_audit_stop() { rsf_core_reflex_audit_stop(); }
void rsf_reflex_driver_sleep(void* device, rsf_backend_log_fn log, void* user)
{
    rsf_core_reflex_driver_sleep(device, log, user, rsf::emit_streamline_log);
}
#else
void rsf_reflex_audit_start(rsf_backend_log_fn, void*) {}
void rsf_reflex_audit_stop() {}
void rsf_reflex_driver_sleep(void*, rsf_backend_log_fn, void*) {}
#endif
