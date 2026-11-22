// SPDX-License-Identifier: MIT
#pragma once
#include "sdk_fg_options.h"
#include <sl_dlss_g.h>

/* Native DLSS-G SDK layout assertions. sl.h is C++ only, so the
   streamline_selected_device_contract fixture compiles this header. */
static_assert(sizeof(sl::DLSSGState) == sizeof(rsf_slvk_sdk_fg_state) &&
    alignof(sl::DLSSGState) == alignof(rsf_slvk_sdk_fg_state));
static_assert(sl::DLSSGState::s_structType.data1 == 0xcc8ac8e1);

static_assert(sizeof(sl::DLSSGOptions) == sizeof(rsf_slvk_sdk_fg_options) &&
    alignof(sl::DLSSGOptions) == alignof(rsf_slvk_sdk_fg_options) &&
    __is_trivially_copyable(sl::DLSSGOptions));
static_assert(sl::kSDKVersion == 0x0002000e0001fedcull);
static_assert(sl::DLSSGOptions::s_structType.data1 == 0xfac5f1cb);
static_assert(uint32_t(sl::DLSSGMode::eOff) == 0 && uint32_t(sl::DLSSGMode::eOn) == 1);
static_assert(uint32_t(sl::DLSSGFlags::eRetainResourcesWhenOff) == 8);
static_assert(uint32_t(sl::DLSSGQueueParallelismMode::eBlockNoClientQueues) == 1);
static_assert(sl::Boolean::eFalse == 0 && sl::Boolean::eTrue == 1 && sl::Boolean::eInvalid == 2);

#if defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Winvalid-offsetof"
#endif
#if defined(__clang__) || defined(__GNUC__)
#define RSF_FG_OFFSET(type, member) __builtin_offsetof(type, member)
#else
#define RSF_FG_OFFSET(type, member) offsetof(type, member)
#endif
#define RSF_FG_FIELD(vendor, wire) \
    static_assert(RSF_FG_OFFSET(sl::DLSSGOptions, vendor) == RSF_FG_OFFSET(rsf_slvk_sdk_fg_options, wire))
RSF_FG_FIELD(mode, mode);
RSF_FG_FIELD(numFramesToGenerate, generated);
RSF_FG_FIELD(flags, flags);
RSF_FG_FIELD(dynamicResWidth, dynamic_width);
RSF_FG_FIELD(dynamicResHeight, dynamic_height);
RSF_FG_FIELD(numBackBuffers, buffers);
RSF_FG_FIELD(mvecDepthWidth, guide_width);
RSF_FG_FIELD(mvecDepthHeight, guide_height);
RSF_FG_FIELD(colorWidth, color_width);
RSF_FG_FIELD(colorHeight, color_height);
RSF_FG_FIELD(colorBufferFormat, color_format);
RSF_FG_FIELD(mvecBufferFormat, motion_format);
RSF_FG_FIELD(depthBufferFormat, depth_format);
RSF_FG_FIELD(hudLessBufferFormat, hudless_format);
RSF_FG_FIELD(uiBufferFormat, ui_format);
RSF_FG_FIELD(onErrorCallback, reserved_callback);
RSF_FG_FIELD(bReserved15, reserved_boolean);
RSF_FG_FIELD(queueParallelismMode, queue_parallelism);
RSF_FG_FIELD(enableUserInterfaceRecomposition, ui_recomposition);
RSF_FG_FIELD(dynamicTargetFrameRate, target_rate);
#define RSF_FG_STATE(vendor, wire) \
    static_assert(RSF_FG_OFFSET(sl::DLSSGState, vendor) == RSF_FG_OFFSET(rsf_slvk_sdk_fg_state, wire))
RSF_FG_STATE(estimatedVRAMUsageInBytes, estimated_vram);
RSF_FG_STATE(status, status);
RSF_FG_STATE(minWidthOrHeight, min_dimension);
RSF_FG_STATE(numFramesActuallyPresented, actual_presents);
RSF_FG_STATE(numFramesToGenerateMax, max_generated);
RSF_FG_STATE(bReserved4, reserved);
RSF_FG_STATE(bIsVsyncSupportAvailable, vsync);
RSF_FG_STATE(inputsProcessingCompletionFence, inputs_fence);
RSF_FG_STATE(lastPresentInputsProcessingCompletionFenceValue, inputs_fence_value);
RSF_FG_STATE(bIsDynamicMFGSupported, dynamic_mfg);
#undef RSF_FG_STATE
#undef RSF_FG_FIELD
#undef RSF_FG_OFFSET
#if defined(__GNUC__)
#pragma GCC diagnostic pop
#endif
