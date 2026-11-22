/* SPDX-License-Identifier: MIT */
#pragma once
#include "vulkan_sdk_native.h"

#if defined(RSF_HAVE_FFX_VK)
#include <rescaleframe/native_sr_vulkan.h>
#include <windows.h>
#include <ffx_api/ffx_api.h>

#ifdef __cplusplus
extern "C" {
#endif

/* FidelityFX context exports resolved from the loaded SDK module. */
typedef struct rsf_vkffx_exports {
    PfnFfxCreateContext create;
    PfnFfxDestroyContext destroy;
    PfnFfxQuery query;
    PfnFfxDispatch dispatch;
} rsf_vkffx_exports;

rsf_vk_function rsf_vkffx_symbol(void* module, const char* name);

#ifdef __cplusplus
}
#endif
#endif
