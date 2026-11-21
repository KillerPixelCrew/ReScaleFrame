// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

struct rsf_vkffx_loader {
    void* (*load)(const uint16_t*);
    void (*unload)(void*);
};
static_assert(sizeof(rsf_vkffx_loader) == 2 * sizeof(void*));

extern "C" void* rsf_core_vkffx_load(const uint16_t*);
extern "C" void rsf_core_vkffx_unload(void*);
inline constexpr rsf_vkffx_loader rsf_vkffx_system_loader{rsf_core_vkffx_load, rsf_core_vkffx_unload};

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
