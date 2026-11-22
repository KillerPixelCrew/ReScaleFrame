// SPDX-License-Identifier: MIT
#include "native_latency_vulkan_platform.h"

// A borrowed callback can enter foreign C++ code. Catch there before returning to Rust.
extern "C" int32_t rsf_sllatency_invoke_checked(rsf_slvk_callback callback, void* module,
    uintptr_t token, void* user) noexcept try {
    return callback(module, token, user);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
