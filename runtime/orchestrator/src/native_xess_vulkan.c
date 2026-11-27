/* SPDX-License-Identifier: MIT */
/* XeSS Vulkan SR SDK layout checks. Rust (vulkan_sr/xess.rs and xess/native.rs)
   owns the public entry points, export lookup, the SDK session, typed SDK calls,
   contexts and teardown. This unit checks the SDK C layouts and values that the
   Rust owner mirrors while those records are hand-written. */
#include <rescaleframe/native_xess_vulkan_requirements.h>

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
#endif
