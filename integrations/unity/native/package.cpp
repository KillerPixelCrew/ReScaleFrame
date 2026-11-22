// SPDX-License-Identifier: MIT
// Unity SDK registration, managed exports and native Runtime leaves for the Rust package.
#include "package.h"
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/native_cpu.h>
#include "../../windows/window_input.h"
#include "bridge.h"
#include "vulkan_bridge.h"
#include "preview.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <IUnityGraphics.h>
#include <wrl/client.h>

extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_load(IUnityInterfaces*);
extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_unload();
extern "C" int rsf_unity_set_engine_spatial(uint32_t, uint32_t) noexcept;

extern "C" uint64_t rsf_unity_package_session_seed() noexcept;
extern "C" void rsf_unity_package_timestamp(uint64_t*, uint64_t*) noexcept;
extern "C" uint32_t rsf_unity_package_local_app_data(wchar_t*, uint32_t) noexcept;
extern "C" void rsf_unity_package_create_directory(const wchar_t*) noexcept;
extern "C" void* rsf_unity_package_log_open(const wchar_t*) noexcept;
extern "C" void rsf_unity_package_log_write(void*, const char*) noexcept;
extern "C" void rsf_unity_package_log_close(void*) noexcept;
extern "C" void* rsf_unity_package_player_window() noexcept;
extern "C" uint32_t rsf_unity_package_window_class(void* hwnd) noexcept;
extern "C" uint32_t rsf_unity_package_subclass(void* hwnd) noexcept;
extern "C" void rsf_unity_package_restore(void* hwnd) noexcept;

// package_abi.rs mirrors these layouts and constants.
static_assert(sizeof(rsf_unity_package_settings) == 100 && sizeof(rsf_unity_package_status) == 184);
static_assert(sizeof(rsf_unity_frame_counters) == 40 && sizeof(rsf_unity_package_platform) == 424);
static_assert(sizeof(rsf_native_fg_status) == 352 && sizeof(rsf_native_latency_status) == 152);
static_assert(sizeof(rsf_native_fg_options) == 24 && RSF_BACKEND_OK == 0);
static_assert(RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_SR_NONE == 0);
static_assert(sizeof(rsf_unity_package_view_setup) == 88 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4);
// generation.rs builds the install; runtime_abi.rs mirrors rsf_fg12_setup.
static_assert(sizeof(rsf_fg12_setup) == 152 && RSF_DLSS_ENGINE_UNITY == 2);
static_assert(sizeof(rsf_sr12_setup) == 216 && sizeof(rsf_sr12_plan_result) == 24);
// services.rs mirrors rsfu_input's results and the host service layout.
static_assert(RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9);
static_assert(sizeof(rsf_game_host_services) == 104 && sizeof(rsf_unity_native_api) == 72 && sizeof(wchar_t) == 2);

// The SDK enum crosses as its raw value. Rust decides which events it handles.
static_assert(kUnityGfxDeviceEventInitialize == RSF_UNITY_DEVICE_EVENT_INITIALIZE);
static_assert(kUnityGfxDeviceEventShutdown == RSF_UNITY_DEVICE_EVENT_SHUTDOWN);

namespace {
using Microsoft::WRL::ComPtr;
void UNITY_INTERFACE_API device_event(UnityGfxDeviceEventType type)
{
    rsf_unity_package_device_event(static_cast<uint32_t>(type));
}
void* graphics_interface(void* pointer) noexcept
{
    auto* interfaces = static_cast<IUnityInterfaces*>(pointer);
    return interfaces ? interfaces->Get<IUnityGraphics>() : nullptr;
}
void preload(void* pointer) noexcept { rsf_uvk_preload(static_cast<IUnityInterfaces*>(pointer)); }
void device_callback(void* pointer, uint32_t subscribe) noexcept
{
    auto* graphics = static_cast<IUnityGraphics*>(pointer);
    if (subscribe) graphics->RegisterDeviceEventCallback(device_event);
    else graphics->UnregisterDeviceEventCallback(device_event);
}
uint32_t bridge_open(uint32_t is_editor) noexcept try
{
    return rsf_unity_package_bridge_open(is_editor != 0) ? 1u : 0u;
} catch (...) { return 0; }
int fg_window(const rsf_game_render_pass* pass, uint32_t catch_exception) noexcept
{
    // The player call keeps its noexcept boundary. The preview observer can report refusal.
    if (!catch_exception) {
        rsf_fg12_window(pass);
        return 1;
    }
    try {
        rsf_fg12_window(pass);
        return 1;
    } catch (...) {
        return 0;
    }
}
}
extern "C" uint32_t rsf_unity_package_acquire_graphics(void** device, void** queue) noexcept
{
    ComPtr<ID3D12Device> owning_device;
    ComPtr<ID3D12CommandQueue> owning_queue;
    if (!rsf_unity_package_graphics(owning_device, owning_queue) || !owning_device || !owning_queue) return 0;
    *device = owning_device.Detach();
    *queue = owning_queue.Detach();
    return 1;
}
extern "C" uint32_t rsf_unity_package_choose_fg(uint32_t vendor, uint32_t software) noexcept
{
    return rsf_fg_default_backend(vendor, software);
}
extern "C" uint32_t rsf_unity_package_create_planned(const rsf_sr12_setup* setup,
    rsf_sr12_plan_result* plan, rsf_sr12** output, int32_t* result) noexcept try
{
    *result = rsf_sr12_create_planned(setup, plan, output);
    return 1;
} catch (...) { return 0; }
extern "C" void* rsf_unity_package_streamline_host() noexcept { return rsf_d3d11_present_host(); }

namespace {
constexpr rsf_unity_package_platform platform{
    [](const rsf_sharpening_options* options) noexcept -> uint32_t { return rsf::sharpening_settings::valid(*options) ? 1u : 0u; },
    [](uint32_t preset) noexcept -> uint32_t { return rsf_dlss_preset_valid(preset) ? 1u : 0u; },
    rsf_unity_set_engine_spatial,
    [](const rsf_native_fg_options* options) noexcept { rsf_fg12_options(options); },
    [](rsf_native_fg_status* output) noexcept { return rsf_fg12_status(output); },
    [](rsf_native_latency_status* output) noexcept { return rsf_native_latency_status_get(output); },
    []() noexcept { return rsf_d3d11_present_backend(); },
    []() noexcept { return rsf_d3d11_present_switch_result(); },
    []() noexcept { return rsf_d3d11_present_generation(); },
    [](uint32_t enabled) noexcept { rsf_unity_bridge_activate(enabled != 0); },
    []() noexcept -> uint32_t { return rsf_unity_package_bridge_try_close() ? 1u : 0u; },
    [](uint64_t session) noexcept -> uint32_t { return rsf_fg12_end_session(session) ? 1u : 0u; },
    rsf_sr12_quality_supported,
    rsf_unity_package_create_view,
    [](rsf_sharpening_options* output) noexcept { *output = rsf_sharpening_defaults(); },
    [](rsf_sr12* sr, const rsf_sharpening_options* options) noexcept -> int32_t { return rsf_sr12_set_sharpening(sr, options); },
    [](rsf_sr12* sr, void* commands, const rsf_game_render_pass* pass, uint32_t slot) noexcept -> int32_t {
        return rsf_sr12_evaluate(sr, commands, pass, slot);
    },
    [](rsf_sr12* sr) noexcept { rsf_sr12_destroy(sr); },
    [](uint32_t commit) noexcept { rsf_unity_package_bridge_finish_retirement(commit != 0); },
    [](const rsf_fg12_setup* setup) noexcept -> int32_t { return rsf_fg12_install(setup); },
    []() noexcept -> uint32_t { return rsf_d3d11_present_has_owner() ? 1u : 0u; },
    [](uint32_t backend) noexcept -> int32_t { return rsf_d3d11_present_request(backend); },
    rsf_unity_package_default_fg,
    bridge_open,
    rsf_unity_package_session_seed,
    [](const rsf_game_host_services* services, rsf_unity_native_api* api) noexcept -> uint32_t {
        return rsf_unity_bridge_prepare(*services, api) ? 1u : 0u;
    },
    rsf_unity_package_local_app_data,
    rsf_unity_package_create_directory,
    rsf_unity_package_log_open,
    rsf_unity_package_log_write,
    rsf_unity_package_log_close,
    rsf_unity_package_player_window,
    []() noexcept { return rsf_unity_preview_parent(); },
    rsf_unity_package_subclass,
    rsf_unity_package_restore,
    rsf_win32_input_kind,
    rsf_unity_package_timestamp,
    [](const rsf_game_cpu_event* event) noexcept { rsf_fg12_cpu(event); },
    [](uint64_t session, uint64_t frame) noexcept { rsf_fg12_submitted(session, frame); },
    fg_window,
    [](void* commands, const rsf_game_render_pass* pass) noexcept { rsf_fg12_hudless(commands, pass); },
    rsf_unity_preview_present,
    rsf_unity_package_texture_desc,
    rsf_unity_package_prepare_preview,
    [](void* window) noexcept -> uint32_t { return rsf_unity_preview_owns(window) ? 1u : 0u; },
    [](void* pointer) noexcept -> uint32_t {
        auto* graphics = static_cast<IUnityGraphics*>(pointer);
        return graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan ? 1u : 0u;
    },
    [](void* window) noexcept -> uint32_t { return rsf_uvk_owns_window(window) ? 1u : 0u; },
    rsf_unity_package_window_class,
    [](void* interfaces) noexcept -> uint32_t {
        if (!interfaces) return 0;
        rsf_unity_package_bridge_load(static_cast<IUnityInterfaces*>(interfaces));
        return 1;
    },
    []() noexcept { rsf_unity_package_bridge_unload(); },
    graphics_interface,
    preload,
    device_callback,
};
}

void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept
{
    rsf_unity_package_bridge_failed(result);
    rsf_unity_package_log(message);
}
bool rsf_unity_package_is_editor() noexcept { return rsf_unity_package_editor() != 0; }

extern "C" __declspec(dllexport) int rsfu_initialize(const char* runtime, const char* engine, const char* id, rsf_unity_native_api* output) noexcept
{
    return rsf_unity_package_initialize(runtime, engine, id, output);
}
extern "C" __declspec(dllexport) int rsfu_configure(const rsf_unity_package_settings* input) noexcept
{
    return rsf_unity_package_configure(input);
}
extern "C" __declspec(dllexport) int rsfu_settings(rsf_unity_package_settings* output) noexcept
{
    return rsf_unity_package_settings_get(output);
}
extern "C" __declspec(dllexport) int rsfu_temporal_ready() noexcept { return rsf_unity_package_temporal_ready() ? 1 : 0; }
extern "C" __declspec(dllexport) int rsfu_surface(uint32_t width, uint32_t height) noexcept
{
    return rsf_uvk_update_window(width, height) ? 1 : 0;
}
extern "C" __declspec(dllexport) int rsfu_preview_region(int left, int top, int width, int height, uint32_t visible) noexcept
{
    return rsf_unity_package_preview_region(left, top, width, height, visible);
}
extern "C" __declspec(dllexport) int rsfu_frame_counters(rsf_unity_frame_counters* output) noexcept
{
    return rsf_unity_package_frame_counters(output);
}

extern "C" __declspec(dllexport) int rsfu_status(rsf_unity_package_status* output) noexcept
{
    return rsf_unity_package_status_get(output);
}

extern "C" __declspec(dllexport) int rsfu_input(uint64_t frame, uint32_t kinds) noexcept
{
    return rsf_unity_package_input(frame, kinds);
}

extern "C" __declspec(dllexport) int rsfu_shutdown() noexcept
{
    return rsf_unity_package_shutdown() ? 1 : 0;
}

extern "C" void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    rsf_unity_package_plugin_load(&platform, interfaces);
}
extern "C" void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginUnload()
{
    rsf_unity_package_plugin_unload();
}
