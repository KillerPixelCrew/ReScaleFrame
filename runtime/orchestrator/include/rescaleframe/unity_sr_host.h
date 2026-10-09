// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <stdint.h>
// Streamline identity of the Unity players the shared plugin supports, for every Unity entry point
// that registers with Streamline (the engine type is RSF_DLSS_ENGINE_UNITY).
#define RSF_UNITY_ENGINE_VERSION "6000.3"
#define RSF_UNITY_PROJECT_ID "57a42c7e-faf0-4bda-a9f9-892870948ac1"
#ifdef __cplusplus
extern "C" {
#endif
// Explicit research entry point, outside DllMain. Reads an absolute UTF-16 INI path, prepares
// the Unity plugin, then waits for its actual render callback to initialize the SR device.
RSF_RUNTIME_API uint32_t __stdcall rsf_unity_sr_start(const wchar_t* configuration_path);
// Busy preserves the runtime and plugin while managed/native/GPU callbacks remain owned.
RSF_RUNTIME_API uint32_t __stdcall rsf_unity_sr_stop(void* unused);
#ifdef __cplusplus
}
#endif
