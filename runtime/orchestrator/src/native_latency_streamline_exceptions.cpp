// SPDX-License-Identifier: MIT
#include "native_latency_vulkan_platform.h"

extern "C" int rsf_vk_sl_initialize_features(const wchar_t*, const char*, uint32_t, uint32_t, uint32_t);

// Keep exceptions from the Streamline bridge and the borrowed host callback inside C++.
extern "C" int32_t rsf_sllatency_initialize_checked(const uint16_t* directory, const char* project, uint32_t reflex,
    uint32_t fg, uint32_t sr, int32_t* result) noexcept try {
    *result = rsf_vk_sl_initialize_features(reinterpret_cast<const wchar_t*>(directory),
        project, reflex, fg, sr);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" int32_t rsf_sllatency_invoke_checked(const rsf_slvk_call* call, void* module, uintptr_t token) noexcept try {
    return call->callback(module, token, call->user);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
