// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// Explicit research entry point, outside DllMain. Reads an absolute UTF-16 INI path, prepares
// the Unity plugin, then waits for its actual render callback to initialize the SR device.
// Serialized with stop. Zero means lifecycle activation, not an accepted SR image. Return codes:
// 1 existing owner/path, 2 policy values, 3 paths, 4 log, 5 executable path, 6 hash, 7 module,
// 8 policy export/refusal, 9 retained busy owner, 10 plugin lifecycle refusal, 11 exception.
RSF_RUNTIME_API uint32_t __stdcall rsf_unity_sr_start(const wchar_t* configuration_path);
// Busy preserves the runtime and plugin while managed/native/GPU callbacks remain owned.
// Zero means stopped/already absent, 1 lifecycle still busy, 2 exception. Retry a busy owner;
// do not unload the runtime or release callback user storage while that owner remains live.
RSF_RUNTIME_API uint32_t __stdcall rsf_unity_sr_stop(void* unused);
#ifdef __cplusplus
}
#endif
