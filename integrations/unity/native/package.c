/* SPDX-License-Identifier: MIT */
/* Unity SDK registration, the managed rsfu_* exports and the platform table that the Rust package
   (package_state.rs) adopts at plugin load. Entries without a body here are Runtime or Rust
   functions; the wrappers only adapt result types. */
#include "package_state.h"
#include "bridge.h"
#include "preview.h"
#include "vulkan_bridge.h"
#include "vulkan_foreign.h"
#include "../../windows/window_input.h"
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/dlss_preset.h>
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <windows.h>
#define RSF_UNITY_SDK_LEAF rsf_unity_package
#include "unity_sdk.h"

/* package_state/windows.rs. */
uint64_t rsf_unity_package_session_seed(void);
void rsf_unity_package_timestamp(uint64_t* counter, uint64_t* frequency);
uint32_t rsf_unity_package_local_app_data(wchar_t* output, uint32_t capacity);
void rsf_unity_package_create_directory(const wchar_t* path);
void* rsf_unity_package_log_open(const wchar_t* path);
void rsf_unity_package_log_write(void* file, const char* text);
void rsf_unity_package_log_close(void* file);
void* rsf_unity_package_player_window(void);
uint32_t rsf_unity_package_window_class(void* hwnd);
uint32_t rsf_unity_package_subclass(void* hwnd);
void rsf_unity_package_restore(void* hwnd);

/* package_abi.rs mirrors these layouts and constants. */
_Static_assert(sizeof(rsf_unity_package_settings) == 100 && sizeof(rsf_unity_package_status) == 184, "package ABI");
_Static_assert(sizeof(rsf_unity_frame_counters) == 40 && sizeof(rsf_unity_package_platform) == 424, "package ABI");
_Static_assert(sizeof(rsf_native_fg_status) == 352 && sizeof(rsf_native_latency_status) == 152, "Runtime status");
_Static_assert(sizeof(rsf_native_fg_options) == 24 && RSF_BACKEND_OK == 0, "Runtime options");
_Static_assert(RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_SR_NONE == 0, "Runtime constants");
_Static_assert(sizeof(rsf_unity_package_view_setup) == 88 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4,
    "view setup");
/* generation.rs builds the install; runtime_abi.rs mirrors rsf_fg12_setup. */
_Static_assert(sizeof(rsf_fg12_setup) == 152 && RSF_DLSS_ENGINE_UNITY == 2, "generation install");
_Static_assert(sizeof(rsf_sr12_setup) == 216 && sizeof(rsf_sr12_plan_result) == 24, "SR setup");
/* services.rs mirrors rsfu_input's results and the host service layout. */
_Static_assert(RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9, "results");
_Static_assert(sizeof(rsf_game_host_services) == 104 && sizeof(rsf_unity_native_api) == 72 && sizeof(wchar_t) == 2,
    "host services");

/* The SDK enum crosses as its raw value. Rust decides which events it handles. */
_Static_assert(kUnityGfxDeviceEventInitialize == RSF_UNITY_DEVICE_EVENT_INITIALIZE, "initialize event");
_Static_assert(kUnityGfxDeviceEventShutdown == RSF_UNITY_DEVICE_EVENT_SHUTDOWN, "shutdown event");

static void UNITY_INTERFACE_API device_event(UnityGfxDeviceEventType type)
{
    rsf_unity_package_device_event((uint32_t)type);
}

static void* graphics_interface(void* pointer)
{
    IUnityInterfaces* interfaces = (IUnityInterfaces*)pointer;
    return interfaces ? RSF_UNITY_GET(interfaces, IUnityGraphics) : NULL;
}

static void device_callback(void* pointer, uint32_t subscribe)
{
    IUnityGraphics* graphics = (IUnityGraphics*)pointer;
    if (subscribe) graphics->RegisterDeviceEventCallback(device_event);
    else graphics->UnregisterDeviceEventCallback(device_event);
}

static uint32_t renderer_vulkan(void* pointer)
{
    IUnityGraphics* graphics = (IUnityGraphics*)pointer;
    return graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan ? 1u : 0u;
}

static uint32_t sharpening_valid(const rsf_sharpening_options* options)
{
    return options->struct_size >= sizeof(*options) && rsf_sharpening_core_valid(*options) != 0 ? 1u : 0u;
}

static uint32_t dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset) ? 1u : 0u; }
static uint32_t end_session(uint64_t session) { return rsf_fg12_end_session(session) ? 1u : 0u; }
static void sharpening_defaults(rsf_sharpening_options* output) { *output = rsf_sharpening_defaults(); }
static uint32_t present_has_owner(void) { return rsf_d3d11_present_has_owner() ? 1u : 0u; }
static uint32_t preview_owns(void* window) { return rsf_unity_preview_owns(window) ? 1u : 0u; }
static uint32_t vulkan_owns(void* window) { return rsf_uvk_bridge_owns_window(window) ? 1u : 0u; }

static const rsf_unity_package_platform platform = {
    sharpening_valid,
    dlss_preset_valid,
    rsf_unity_set_engine_spatial,
    rsf_fg12_options,
    rsf_fg12_status,
    rsf_native_latency_status_get,
    rsf_d3d11_present_backend,
    rsf_d3d11_present_switch_result,
    rsf_d3d11_present_generation,
    rsf_unity_bridge_activate,
    rsf_unity_package_bridge_try_close,
    end_session,
    rsf_sr12_quality_supported,
    rsf_unity_package_create_view,
    sharpening_defaults,
    rsf_sr12_set_sharpening,
    rsf_sr12_evaluate,
    rsf_sr12_destroy,
    rsf_unity_package_bridge_finish_retirement,
    rsf_fg12_install,
    present_has_owner,
    rsf_d3d11_present_request,
    rsf_unity_package_default_fg,
    rsf_unity_package_bridge_open,
    rsf_unity_package_session_seed,
    rsf_unity_package_bridge_prepare,
    rsf_unity_package_local_app_data,
    rsf_unity_package_create_directory,
    rsf_unity_package_log_open,
    rsf_unity_package_log_write,
    rsf_unity_package_log_close,
    rsf_unity_package_player_window,
    rsf_unity_preview_parent,
    rsf_unity_package_subclass,
    rsf_unity_package_restore,
    rsf_win32_input_kind,
    rsf_unity_package_timestamp,
    rsf_fg12_cpu,
    rsf_fg12_submitted,
    rsf_fg12_window,
    rsf_fg12_hudless,
    rsf_unity_preview_present,
    rsf_unity_package_texture_desc,
    rsf_unity_package_prepare_preview,
    preview_owns,
    renderer_vulkan,
    vulkan_owns,
    rsf_unity_package_window_class,
    rsf_unity_package_bridge_load,
    rsf_unity_package_bridge_unload,
    graphics_interface,
    rsf_uvk_preload,
    device_callback,
};

__declspec(dllexport) int rsfu_initialize(const char* runtime, const char* engine, const char* id,
    rsf_unity_native_api* output)
{
    return rsf_unity_package_initialize(runtime, engine, id, output);
}

__declspec(dllexport) int rsfu_configure(const rsf_unity_package_settings* input)
{
    return rsf_unity_package_configure(input);
}

__declspec(dllexport) int rsfu_settings(rsf_unity_package_settings* output)
{
    return rsf_unity_package_settings_get(output);
}

__declspec(dllexport) int rsfu_temporal_ready(void) { return rsf_unity_package_temporal_ready() ? 1 : 0; }

__declspec(dllexport) int rsfu_surface(uint32_t width, uint32_t height)
{
    return rsf_uvk_bridge_update_window(width, height) ? 1 : 0;
}

__declspec(dllexport) int rsfu_preview_region(int left, int top, int width, int height, uint32_t visible)
{
    return rsf_unity_package_preview_region(left, top, width, height, visible);
}

__declspec(dllexport) int rsfu_frame_counters(rsf_unity_frame_counters* output)
{
    return rsf_unity_package_frame_counters(output);
}

__declspec(dllexport) int rsfu_status(rsf_unity_package_status* output)
{
    return rsf_unity_package_status_get(output);
}

__declspec(dllexport) int rsfu_input(uint64_t frame, uint32_t kinds)
{
    return rsf_unity_package_input(frame, kinds);
}

__declspec(dllexport) int rsfu_shutdown(void) { return rsf_unity_package_shutdown() ? 1 : 0; }

void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    rsf_unity_package_plugin_load(&platform, interfaces);
}

void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginUnload(void)
{
    rsf_unity_package_plugin_unload();
}
