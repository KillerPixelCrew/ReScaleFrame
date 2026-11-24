/* SPDX-License-Identifier: MIT */
/* Unity SDK registration. The Rust package (package_state.rs) binds the Runtime and its own calls
   directly and exports the managed rsfu_* entry points (bridge.def); this file keeps the Unity SDK calls. */
#include "package_state.h"
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <windows.h>
#define RSF_UNITY_SDK_LEAF rsf_unity_package
#include "unity_sdk.h"

/* The managed contract (Runtime/Native.cs) fixes these layouts. */
_Static_assert(sizeof(rsf_unity_package_settings) == 100 && sizeof(rsf_unity_package_status) == 184, "package ABI");
_Static_assert(sizeof(rsf_unity_frame_counters) == 40, "package ABI");
/* Rust restates these cast macros, which bindgen does not evaluate. */
_Static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9,
    "results");
_Static_assert(RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_SR_NONE == 0, "Runtime constants");
_Static_assert(RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4, "screens");
_Static_assert(RSF_DLSS_ENGINE_UNITY == 2, "generation install");

/* The SDK enum crosses as its raw value. Rust decides which events it handles. */
_Static_assert(kUnityGfxDeviceEventInitialize == RSF_UNITY_DEVICE_EVENT_INITIALIZE, "initialize event");
_Static_assert(kUnityGfxDeviceEventShutdown == RSF_UNITY_DEVICE_EVENT_SHUTDOWN, "shutdown event");

static void UNITY_INTERFACE_API device_event(UnityGfxDeviceEventType type)
{
    rsf_unity_package_device_event((uint32_t)type);
}

void* rsf_unity_package_graphics_interface(void* pointer)
{
    IUnityInterfaces* interfaces = (IUnityInterfaces*)pointer;
    return interfaces ? RSF_UNITY_GET(interfaces, IUnityGraphics) : NULL;
}

void rsf_unity_package_device_callback(void* pointer, uint32_t subscribe)
{
    IUnityGraphics* graphics = (IUnityGraphics*)pointer;
    if (subscribe) graphics->RegisterDeviceEventCallback(device_event);
    else graphics->UnregisterDeviceEventCallback(device_event);
}

uint32_t rsf_unity_package_renderer_vulkan(void* pointer)
{
    IUnityGraphics* graphics = (IUnityGraphics*)pointer;
    return graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan ? 1u : 0u;
}

void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    rsf_unity_package_plugin_load(interfaces);
}

void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginUnload(void)
{
    rsf_unity_package_plugin_unload();
}
