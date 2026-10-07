// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/unity_bridge.h>

// Load adjacent Harmony/helper assemblies into the identified Unity script domain on a fresh,
// owned Mono-attached worker. helper/api/reason are required and borrowed until the worker joins.
// On success caches managed Stop; reason receives immutable diagnostic text on either outcome.
bool rsf_unity_mono_start(const wchar_t* helper, rsf_unity_native_api* api, const char** reason) noexcept;
// Invoke managed Stop on an owned attached worker. Returns false while producers refuse cleanup;
// retry without unloading native callbacks. Managed assemblies remain loaded but inert on success.
bool rsf_unity_mono_stop() noexcept;
