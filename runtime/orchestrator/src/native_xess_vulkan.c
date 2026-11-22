/* SPDX-License-Identifier: MIT */
/* XeSS Vulkan SR public exports. Rust (vulkan_sr/xess.rs and xess/native.rs)
   owns export lookup, the SDK session, typed SDK calls, contexts and teardown.
   This leaf selects the table for SDK-enabled builds and checks the SDK C
   layouts and values that the Rust owner mirrors. */
#include "native_xess_vulkan.h"

#if defined(RSF_HAVE_XESS_VK)
#define VK_NO_PROTOTYPES
#include <stdbool.h> /* xess.h uses bool without including it in C. */
#include <stddef.h>
#include <windows.h>
#include <xess/xess_vk.h>

_Static_assert(sizeof(rsf_xessvk_setup) == 16, "XeSS requirements setup layout");
_Static_assert(sizeof(xess_vk_image_view_info) == 48 && offsetof(xess_vk_image_view_info, format) == 36,
    "XeSS image view layout");
_Static_assert(sizeof(xess_vk_init_params_t) == 64 && offsetof(xess_vk_init_params_t, qualitySetting) == 8,
    "XeSS init layout");
_Static_assert(sizeof(xess_vk_execute_params_t) == 360 && offsetof(xess_vk_execute_params_t, resetHistory) == 300,
    "XeSS execute layout");
_Static_assert(sizeof(xess_version_t) == 8 && sizeof(xess_2d_t) == 8, "XeSS value layouts");
_Static_assert(VK_IMAGE_ASPECT_COLOR_BIT == 1, "Vulkan colour aspect");
/* Values mirrored by the Rust owner. */
_Static_assert(XESS_RESULT_SUCCESS == 0 && XESS_RESULT_ERROR_UNSUPPORTED_DEVICE == -1 &&
    XESS_RESULT_ERROR_UNSUPPORTED_DRIVER == -2 && XESS_RESULT_ERROR_NOT_IMPLEMENTED == -7 &&
    XESS_RESULT_ERROR_UNSUPPORTED == -10, "XeSS results");
_Static_assert(XESS_INIT_FLAG_INVERTED_DEPTH == 1 << 1 && XESS_INIT_FLAG_JITTERED_MV == 1 << 7 &&
    XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE == 1 << 8, "XeSS init flags");
_Static_assert(XESS_QUALITY_SETTING_AA == 106 && XESS_QUALITY_SETTING_QUALITY == 103 &&
    XESS_QUALITY_SETTING_BALANCED == 102 && XESS_QUALITY_SETTING_PERFORMANCE == 101 &&
    XESS_QUALITY_SETTING_ULTRA_PERFORMANCE == 100 && XESS_QUALITY_SETTING_ULTRA_QUALITY == 104,
    "XeSS quality values");
_Static_assert(VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100,
    "Vulkan formats");
_Static_assert(VK_IMAGE_LAYOUT_GENERAL == 1 && VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 &&
    VK_IMAGE_USAGE_TRANSFER_DST_BIT == 2 && VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8,
    "Vulkan layouts and usage");
#endif

RSF_RUNTIME_API rsf_backend_result rsf_xessvk_initialize(const rsf_xessvk_setup* setup) {
    return rsf_core_xessvk_initialize(RSF_XESSVK_NATIVE_PLATFORM, setup);
}

RSF_RUNTIME_API rsf_backend_result rsf_xessvk_instance_requirements(uint32_t* count, const char* const** names,
    uint32_t* minimum_api) {
    return rsf_core_xessvk_instance_requirements(RSF_XESSVK_NATIVE_PLATFORM, count, names, minimum_api);
}

RSF_RUNTIME_API rsf_backend_result rsf_xessvk_device_requirements(void* instance, void* physical,
    uint32_t* count, const char* const** names, void** features) {
    return rsf_core_xessvk_device_requirements(RSF_XESSVK_NATIVE_PLATFORM, instance, physical, count, names,
        features);
}

RSF_RUNTIME_API rsf_backend_result rsf_xessvk_shutdown(void) {
    return rsf_core_xessvk_shutdown(RSF_XESSVK_NATIVE_PLATFORM);
}
