// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_sdk_native.h"

#if defined(RSF_HAVE_FFX_VK)
#include <windows.h>
#include <ffx_api/ffx_api.h>

struct rsf_vkffx_exports {
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxQuery query{};
    PfnFfxDispatch dispatch{};

    bool resolve(HMODULE module) noexcept {
        create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(module, "ffxCreateContext"));
        destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(module, "ffxDestroyContext"));
        query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(module, "ffxQuery"));
        dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(module, "ffxDispatch"));
        return create && destroy && query && dispatch;
    }
};
#endif
