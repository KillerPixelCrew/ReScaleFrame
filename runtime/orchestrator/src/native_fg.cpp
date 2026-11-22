// SPDX-License-Identifier: MIT
#include "native_fg_cpu_platform.h"
#include "latency_antilag.h"
#include <rescaleframe/game_api.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>

extern "C" int32_t rsf_d3d11_present_pace_for_generation(uint64_t, rsf_pacing_owner, uint64_t);

namespace {
using Microsoft::WRL::ComPtr;

static_assert(RSF_NFG_CPU_NOT_READY == RSF_BACKEND_ERROR_NOT_READY);
static_assert(RSF_NFG_CPU_INVALID == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
static_assert(RSF_NFG_CPU_INVALID == RSF_ERROR_INVALID_ARGUMENT);
static_assert(RSF_NFG_CPU_ABI_MISMATCH == RSF_ERROR_ABI_MISMATCH);
static_assert(RSF_NFG_CPU_SIM_START == RSF_LATENCY_SIMULATION_START);
static_assert(RSF_NFG_CPU_SIM_END == RSF_LATENCY_SIMULATION_END);
static_assert(RSF_NFG_CPU_SUBMIT_START == RSF_LATENCY_RENDER_SUBMIT_START);
static_assert(RSF_NFG_CPU_SUBMIT_END == RSF_LATENCY_RENDER_SUBMIT_END);
static_assert(RSF_NFG_CPU_PRESENT_START == RSF_LATENCY_PRESENT_START);
static_assert(RSF_NFG_CPU_PRESENT_END == RSF_LATENCY_PRESENT_END);

uint32_t adapter_vendor(void* device) {
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 desc{};
    if (SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(device)->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) &&
        SUCCEEDED(adapter->GetDesc1(&desc)) && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) {
        return desc.VendorId;
    }
    return 0;
}

rsf_streamline_host* host(void* value) { return static_cast<rsf_streamline_host*>(value); }
} // namespace

extern "C" const rsf_nfg_cpu_platform rsf_native_fg_cpu_platform{
    rsf_d3d11_present_has_owner,
    rsf_d3d11_present_generation,
    rsf_d3d11_present_backend,
    rsf_d3d11_present_status_get,
    +[]() -> void* {
        rsf_streamline_graphics graphics{sizeof(graphics)};
        return rsf_d3d11_present_graphics(&graphics) ? graphics.native_device : nullptr;
    },
    adapter_vendor,
    +[]() -> void* { return rsf_d3d11_present_host(); },
    +[](void* sdk, rsf_streamline_latency_status* status) { return rsf_streamline_host_latency_status(host(sdk), status); },
    +[](void* sdk, uint32_t mode, uint32_t interval) { return rsf_streamline_host_reflex(host(sdk), mode, interval); },
    +[](void* sdk, uint64_t source) { return rsf_streamline_host_token(host(sdk), source); },
    +[](void* sdk, uint64_t source) { return rsf_streamline_host_presented(host(sdk), source); },
    +[](void* sdk, uint64_t source) { return rsf_streamline_host_abort(host(sdk), source); },
    +[](void* sdk, uint64_t through) { return rsf_streamline_host_retire_through(host(sdk), through); },
    +[](void* sdk) { return rsf_streamline_host_reset_sources(host(sdk)); },
    rsf_d3d11_present_acquire,
    rsf_d3d11_present_pace_for_generation,
    rsf_d3d11_present_marker,
    rsf_d3d11_present_input,
    rsf_d3d11_present_receipt,
    rsf_d3d11_present_abort,
    +[]() -> void* { return rsf::latency::create_native_antilag_context(); },
    nullptr, // Native fixture override.
    nullptr, // Native fixture override.
    rsf_frame_allows_fg,
};

extern "C" RSF_RUNTIME_API void rsf_native_fg_rendered_source_set(uint32_t enabled) {
    rsf_native_cpu_rendered_source_set(enabled);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_reflex_ordering_set(uint32_t threaded) {
    rsf_core_native_fg_reflex_ordering_set(threaded);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_stage(const char* stage, uint64_t source, uint64_t frame, int32_t result) {
    rsf_core_native_fg_stage(stage, source, frame, result, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_cpu(const rsf_game_cpu_event* event) {
    rsf_core_native_fg_cpu(event, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass* pass, uint32_t begin) {
    rsf_core_native_fg_frame(pass, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_submission(const rsf_game_render_pass* pass, uint32_t begin) {
    rsf_core_native_fg_submission(pass, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_graphics(uint32_t api) {
    rsf_core_native_latency_graphics(api);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_render(uint64_t source, uint32_t begin) {
    rsf_core_native_latency_render(source, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_present(uint64_t source, void* chain, uint32_t begin) {
    rsf_core_native_latency_present(source, chain, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_status_get(rsf_native_latency_status* out) {
    return rsf_core_native_latency_status_get(out, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_frame_get(rsf_native_latency_frame* out) {
    return rsf_core_native_latency_frame_get(out);
}
extern "C" RSF_RUNTIME_API int32_t rsf_native_latency_window_bind(const rsf_native_latency_window* binding) {
    return rsf_core_native_latency_window_bind(binding, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_input_routes_get(rsf_native_input_routes* out) {
    return rsf_core_native_input_routes_get(out);
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_ping(void* window) {
    return rsf_core_native_latency_ping(window, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_retire(uint64_t session, uint64_t through) {
    rsf_core_native_latency_retire(session, through, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_end_session(uint64_t session) {
    rsf_core_native_latency_end_session(session, &rsf_native_fg_cpu_platform);
}
