// SPDX-License-Identifier: MIT
#pragma once
#include "sdk_records.h"
#include <sl.h>

static_assert(sl::kSDKVersion == 0x0002000e0001fedcull);
static_assert(sl::kFeaturePCL == 4 && sl::kFeatureReflex == 3 &&
    sl::kFeatureDLSS_G == 1000 && sl::kFeatureDLSS == 0);
static_assert(uint32_t(sl::EngineType::eCustom) == 0 && uint32_t(sl::RenderAPI::eVulkan) == 2);
static_assert(uint32_t(sl::LogLevel::eDefault) == 1 && uint32_t(sl::LogLevel::eVerbose) == 2);
static_assert((uint64_t(sl::PreferenceFlags::eUseManualHooking) |
    uint64_t(sl::PreferenceFlags::eUseFrameBasedResourceTagging)) == 132);
static_assert(sl::Boolean::eFalse == 0 && sl::Boolean::eTrue == 1);
static_assert(sizeof(wchar_t) == sizeof(uint16_t));
static_assert(sizeof(sl::BaseStructure) == sizeof(rsf_slvk_sdk_header));
static_assert(sizeof(sl::Preferences) == sizeof(rsf_slvk_sdk_preferences) &&
    alignof(sl::Preferences) == alignof(rsf_slvk_sdk_preferences) &&
    __is_trivially_copyable(sl::Preferences));
static_assert(sizeof(sl::Constants) == sizeof(rsf_slvk_sdk_constants) &&
    alignof(sl::Constants) == alignof(rsf_slvk_sdk_constants) &&
    __is_trivially_copyable(sl::Constants));
static_assert(sizeof(sl::Resource) == sizeof(rsf_slvk_sdk_resource) &&
    alignof(sl::Resource) == alignof(rsf_slvk_sdk_resource) &&
    __is_trivially_copyable(sl::Resource));
static_assert(sizeof(sl::ResourceTag) == sizeof(rsf_slvk_sdk_tag) &&
    alignof(sl::ResourceTag) == alignof(rsf_slvk_sdk_tag) &&
    __is_trivially_copyable(sl::ResourceTag));
static_assert(uint32_t(sl::ResourceType::eTex2d) == 0 &&
    sl::ResourceLifecycle::eOnlyValidNow == 0 && sl::ResourceLifecycle::eValidUntilPresent == 1);
static_assert(sl::kBufferTypeDepth == 0 && sl::kBufferTypeMotionVectors == 1 &&
    sl::kBufferTypeHUDLessColor == 2 && sl::kBufferTypeUIColorAndAlpha == 23);

// The Streamline records inherit a nonempty base. Use the compiler's x64
// offsetof extension here to catch changes in that layout.
#if defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Winvalid-offsetof"
#endif
#if defined(__clang__) || defined(__GNUC__)
#define RSF_SL_FIELD_OFFSET(type, member) __builtin_offsetof(type, member)
#else
#define RSF_SL_FIELD_OFFSET(type, member) offsetof(type, member)
#endif
#define RSF_SL_OFFSET(native, wire, vendor, member) \
    static_assert(RSF_SL_FIELD_OFFSET(sl::native, vendor) == RSF_SL_FIELD_OFFSET(rsf_slvk_sdk_##wire, member))
RSF_SL_OFFSET(Preferences, preferences, showConsole, show_console);
RSF_SL_OFFSET(Preferences, preferences, logLevel, log_level);
RSF_SL_OFFSET(Preferences, preferences, pathsToPlugins, plugin_paths);
RSF_SL_OFFSET(Preferences, preferences, numPathsToPlugins, num_plugin_paths);
RSF_SL_OFFSET(Preferences, preferences, pathToLogsAndData, logs_path);
RSF_SL_OFFSET(Preferences, preferences, allocateCallback, allocate);
RSF_SL_OFFSET(Preferences, preferences, releaseCallback, release);
RSF_SL_OFFSET(Preferences, preferences, logMessageCallback, log);
RSF_SL_OFFSET(Preferences, preferences, flags, flags);
RSF_SL_OFFSET(Preferences, preferences, featuresToLoad, features);
RSF_SL_OFFSET(Preferences, preferences, numFeaturesToLoad, num_features);
RSF_SL_OFFSET(Preferences, preferences, applicationId, application_id);
RSF_SL_OFFSET(Preferences, preferences, engine, engine);
RSF_SL_OFFSET(Preferences, preferences, engineVersion, engine_version);
RSF_SL_OFFSET(Preferences, preferences, projectId, project);
RSF_SL_OFFSET(Preferences, preferences, renderAPI, render_api);
RSF_SL_OFFSET(Constants, constants, cameraViewToClip, view_to_clip);
RSF_SL_OFFSET(Constants, constants, clipToCameraView, clip_to_view);
RSF_SL_OFFSET(Constants, constants, clipToLensClip, clip_to_lens);
RSF_SL_OFFSET(Constants, constants, clipToPrevClip, clip_to_previous);
RSF_SL_OFFSET(Constants, constants, prevClipToClip, previous_to_clip);
RSF_SL_OFFSET(Constants, constants, jitterOffset, jitter);
RSF_SL_OFFSET(Constants, constants, mvecScale, motion_scale);
RSF_SL_OFFSET(Constants, constants, cameraPinholeOffset, pinhole);
RSF_SL_OFFSET(Constants, constants, cameraPos, position);
RSF_SL_OFFSET(Constants, constants, cameraUp, up);
RSF_SL_OFFSET(Constants, constants, cameraRight, right);
RSF_SL_OFFSET(Constants, constants, cameraFwd, forward);
RSF_SL_OFFSET(Constants, constants, cameraNear, near_plane);
RSF_SL_OFFSET(Constants, constants, cameraFar, far_plane);
RSF_SL_OFFSET(Constants, constants, cameraFOV, fov);
RSF_SL_OFFSET(Constants, constants, cameraAspectRatio, aspect);
RSF_SL_OFFSET(Constants, constants, motionVectorsInvalidValue, invalid_motion);
RSF_SL_OFFSET(Constants, constants, depthInverted, depth_inverted);
RSF_SL_OFFSET(Constants, constants, cameraMotionIncluded, motion_includes_camera);
RSF_SL_OFFSET(Constants, constants, motionVectors3D, motion_3d);
RSF_SL_OFFSET(Constants, constants, reset, reset);
RSF_SL_OFFSET(Constants, constants, orthographicProjection, orthographic);
RSF_SL_OFFSET(Constants, constants, motionVectorsDilated, motion_dilated);
RSF_SL_OFFSET(Constants, constants, motionVectorsJittered, motion_jittered);
RSF_SL_OFFSET(Constants, constants, minRelativeLinearDepthObjectSeparation, min_relative_depth_separation);
RSF_SL_OFFSET(Resource, resource, type, type);
RSF_SL_OFFSET(Resource, resource, native, image);
RSF_SL_OFFSET(Resource, resource, memory, memory);
RSF_SL_OFFSET(Resource, resource, view, view);
RSF_SL_OFFSET(Resource, resource, state, layout);
RSF_SL_OFFSET(Resource, resource, width, width);
RSF_SL_OFFSET(Resource, resource, height, height);
RSF_SL_OFFSET(Resource, resource, nativeFormat, format);
RSF_SL_OFFSET(Resource, resource, mipLevels, mips);
RSF_SL_OFFSET(Resource, resource, arrayLayers, layers);
RSF_SL_OFFSET(Resource, resource, gpuVirtualAddress, address);
RSF_SL_OFFSET(Resource, resource, flags, flags);
RSF_SL_OFFSET(Resource, resource, usage, usage);
RSF_SL_OFFSET(Resource, resource, internalFlags, internal_flags);
RSF_SL_OFFSET(Resource, resource, reserved, reserved);
RSF_SL_OFFSET(ResourceTag, tag, resource, resource);
RSF_SL_OFFSET(ResourceTag, tag, type, type);
RSF_SL_OFFSET(ResourceTag, tag, lifecycle, lifecycle);
RSF_SL_OFFSET(ResourceTag, tag, extent, top);
static_assert(offsetof(sl::Extent, top) == 0 && offsetof(sl::Extent, left) == 4 &&
    offsetof(sl::Extent, width) == 8 && offsetof(sl::Extent, height) == 12);
#undef RSF_SL_OFFSET
#undef RSF_SL_FIELD_OFFSET
#if defined(__GNUC__)
#pragma GCC diagnostic pop
#endif
