// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_sdk_native.h"

#if defined(RSF_HAVE_FFX_VK)
#include <rescaleframe/native_sr_vulkan.h>
#include <windows.h>
#include <ffx_api/ffx_api.h>

struct rsf_vkffx_exports {
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxQuery query{};
    PfnFfxDispatch dispatch{};

};

inline rsf_vk_function rsf_vkffx_symbol(void* module, const char* name) noexcept {
    return reinterpret_cast<rsf_vk_function>(GetProcAddress(static_cast<HMODULE>(module), name));
}
#endif
