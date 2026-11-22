// SPDX-License-Identifier: MIT
// Native boundary of the Direct3D DLSS SR facades. Rust builds the Streamline 2.14.1 records and
// calls the SDK's C entry points itself (runtime/orchestrator/src/streamline_sr/sdk.rs). What stays
// native is what only the vendor C++ headers can answer:
//  - the record layouts, struct versions and defaults Rust copies, checked against the pinned
//    headers at build time and once at run time;
//  - sl_security.h, whose definitions are the vendor's signature verification and which defines
//    globals, so it belongs in exactly one translation unit.

#include <rescaleframe/dlss.h>
#include "streamline_load.h"

#if RSF_HAVE_STREAMLINE

#include <windows.h>

#include <d3d12.h>

#include <sl.h>
#include <sl_consts.h>
#include <sl_dlss.h>
#include "streamline_sr_records.h"
#include "../../../vulkan/streamline/sdk_records_checks.h"

// sl_security.h includes <Softpub.h>, which mingw-w64 ships as <softpub.h>. Windows filesystems do
// not care and the reference MSVC build gets the real check; the cross build compiles without it
// and refuses to load rather than pretending it verified anything.
#if __has_include(<Softpub.h>)
#include <sl_security.h>
#define RSF_HAVE_SIGNATURE_CHECK 1
#else
#define RSF_HAVE_SIGNATURE_CHECK 0
#endif

#include <cfloat>
#include <climits>
#include <cstddef>

namespace {

#if defined(__clang__)
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Winvalid-offsetof"
#elif defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Winvalid-offsetof"
#endif

#define RSF_SL_RECORD(sdk, wire) \
    static_assert(sizeof(sl::sdk) == sizeof(wire) && alignof(sl::sdk) == alignof(wire))
#define RSF_SL_AT(sdk, wire, sdk_member, wire_member) \
    static_assert(offsetof(sl::sdk, sdk_member) == offsetof(wire, wire_member))
#define RSF_SL_GUID(sdk, d1, d2, d3, b0, b1, b2, b3, b4, b5, b6, b7)                    \
    static_assert(sl::sdk::s_structType.data1 == d1 && sl::sdk::s_structType.data2 == d2 && \
        sl::sdk::s_structType.data3 == d3 && sl::sdk::s_structType.data4[0] == b0 &&        \
        sl::sdk::s_structType.data4[1] == b1 && sl::sdk::s_structType.data4[2] == b2 &&     \
        sl::sdk::s_structType.data4[3] == b3 && sl::sdk::s_structType.data4[4] == b4 &&     \
        sl::sdk::s_structType.data4[5] == b5 && sl::sdk::s_structType.data4[6] == b6 &&     \
        sl::sdk::s_structType.data4[7] == b7)

// Struct types, in the order the Rust builders use them.
RSF_SL_GUID(Preferences, 0x1ca10965, 0xbf8e, 0x432b, 0x8d, 0xa1, 0x67, 0x16, 0xd8, 0x79, 0xfb, 0x14);
RSF_SL_GUID(Constants, 0xdcd35ad7, 0x4e4a, 0x4bad, 0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe);
RSF_SL_GUID(DLSSOptions, 0x6ac826e4, 0x4c61, 0x4101, 0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8);
RSF_SL_GUID(DLSSOptimalSettings, 0xef1d0957, 0xfd58, 0x4df7, 0xb5, 0x04, 0x8b, 0x69, 0xd8, 0xaa, 0x6b, 0x76);
RSF_SL_GUID(Resource, 0x3a9d70cf, 0x2418, 0x4b72, 0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61);
RSF_SL_GUID(ResourceTag, 0x4c6a5aad, 0xb445, 0x496c, 0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53);
RSF_SL_GUID(ViewportHandle, 0x171b6435, 0x9b3c, 0x4fc8, 0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4);
RSF_SL_GUID(FeatureRequirements, 0x66714097, 0xac6d, 0x4bc6, 0x89, 0x15, 0x1e, 0x0f, 0x55, 0xa6, 0xb6, 0x1f);
RSF_SL_GUID(AdapterInfo, 0x0677315f, 0xa746, 0x4492, 0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4);

RSF_SL_RECORD(ViewportHandle, rsf_sl_sdk_viewport);
static_assert(sizeof(sl::BaseStructure) == offsetof(rsf_sl_sdk_viewport, value));

RSF_SL_RECORD(AdapterInfo, rsf_sl_sdk_adapter);
RSF_SL_AT(AdapterInfo, rsf_sl_sdk_adapter, deviceLUID, luid);
RSF_SL_AT(AdapterInfo, rsf_sl_sdk_adapter, deviceLUIDSizeInBytes, luid_size);
RSF_SL_AT(AdapterInfo, rsf_sl_sdk_adapter, vkPhysicalDevice, vk_physical_device);

static_assert(sizeof(sl::Version) == sizeof(rsf_sl_sdk_version) &&
    offsetof(sl::Version, major) == offsetof(rsf_sl_sdk_version, major) &&
    offsetof(sl::Version, minor) == offsetof(rsf_sl_sdk_version, minor) &&
    offsetof(sl::Version, build) == offsetof(rsf_sl_sdk_version, build));
RSF_SL_RECORD(FeatureRequirements, rsf_sl_sdk_requirements);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, flags, flags);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, maxNumCPUThreads, max_cpu_threads);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, maxNumViewports, max_viewports);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, numRequiredTags, num_required_tags);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, requiredTags, required_tags);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, osVersionDetected, os_detected);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, osVersionRequired, os_required);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, driverVersionDetected, driver_detected);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, driverVersionRequired, driver_required);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumComputeQueuesRequired, vk_compute_queues);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumGraphicsQueuesRequired, vk_graphics_queues);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumDeviceExtensions, vk_num_device_extensions);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkDeviceExtensions, vk_device_extensions);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumInstanceExtensions, vk_num_instance_extensions);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkInstanceExtensions, vk_instance_extensions);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumFeatures12, vk_num_features12);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkFeatures12, vk_features12);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumFeatures13, vk_num_features13);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkFeatures13, vk_features13);
RSF_SL_AT(FeatureRequirements, rsf_sl_sdk_requirements, vkNumOpticalFlowQueuesRequired, vk_optical_flow_queues);

// sharpness is deprecated in the SDK, so its slot is checked through its neighbours.
RSF_SL_RECORD(DLSSOptions, rsf_sl_sdk_dlss_options);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, mode, mode);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, outputWidth, output_width);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, outputHeight, output_height);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, preExposure, pre_exposure);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, exposureScale, exposure_scale);
static_assert(offsetof(sl::DLSSOptions, outputHeight) + 2 * sizeof(uint32_t) ==
    offsetof(sl::DLSSOptions, preExposure) &&
    offsetof(rsf_sl_sdk_dlss_options, output_height) + 2 * sizeof(uint32_t) ==
    offsetof(rsf_sl_sdk_dlss_options, pre_exposure));
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, colorBuffersHDR, hdr);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, indicatorInvertAxisX, invert_axis_x);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, indicatorInvertAxisY, invert_axis_y);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, dlaaPreset, presets);
static_assert(sizeof(sl::DLSSPreset) == sizeof(uint32_t) &&
    offsetof(sl::DLSSOptions, qualityPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 4 &&
    offsetof(sl::DLSSOptions, balancedPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 8 &&
    offsetof(sl::DLSSOptions, performancePreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 12 &&
    offsetof(sl::DLSSOptions, ultraPerformancePreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 16 &&
    offsetof(sl::DLSSOptions, ultraQualityPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 20);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, useAutoExposure, auto_exposure);
RSF_SL_AT(DLSSOptions, rsf_sl_sdk_dlss_options, alphaUpscalingEnabled, alpha);

RSF_SL_RECORD(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, optimalRenderWidth, width);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, optimalRenderHeight, height);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, optimalSharpness, sharpness);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, renderWidthMin, width_min);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, renderHeightMin, height_min);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, renderWidthMax, width_max);
RSF_SL_AT(DLSSOptimalSettings, rsf_sl_sdk_dlss_optimal, renderHeightMax, height_max);

#undef RSF_SL_GUID
#undef RSF_SL_AT
#undef RSF_SL_RECORD
#if defined(__clang__)
#pragma clang diagnostic pop
#elif defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

// Values the Rust SR code carries as literals.
static_assert(sizeof(wchar_t) == sizeof(uint16_t));
static_assert(sl::kFeatureDLSS == 0 && sl::kFeatureCommon == 0xffffffffu);
static_assert(uint64_t(sl::PreferenceFlags::eDisableCLStateTracking) == (1ull << 0) &&
    uint64_t(sl::PreferenceFlags::eUseManualHooking) == (1ull << 2) &&
    uint64_t(sl::PreferenceFlags::eAllowOTA) == (1ull << 3) &&
    uint64_t(sl::PreferenceFlags::eLoadDownloadedPlugins) == (1ull << 6) &&
    uint64_t(sl::PreferenceFlags::eUseFrameBasedResourceTagging) == (1ull << 7));
static_assert(uint32_t(sl::EngineType::eCustom) == 0 && uint32_t(sl::EngineType::eUnreal) == RSF_DLSS_ENGINE_UNREAL &&
    uint32_t(sl::EngineType::eUnity) == RSF_DLSS_ENGINE_UNITY);
static_assert(uint32_t(sl::RenderAPI::eD3D11) == 0 && uint32_t(sl::RenderAPI::eD3D12) == 1);
static_assert(uint32_t(sl::LogLevel::eDefault) == 1 && uint32_t(sl::LogType::eWarn) == 1 &&
    uint32_t(sl::LogType::eError) == 2);
static_assert(uint32_t(sl::Result::eOk) == 0 && uint32_t(sl::Result::eErrorDriverOutOfDate) == 2 &&
    uint32_t(sl::Result::eErrorOSOutOfDate) == 3 && uint32_t(sl::Result::eErrorNoSupportedAdapterFound) == 6);
static_assert(uint32_t(sl::DLSSMode::eOff) == 0 && uint32_t(sl::DLSSMode::eMaxPerformance) == 1 &&
    uint32_t(sl::DLSSMode::eBalanced) == 2 && uint32_t(sl::DLSSMode::eMaxQuality) == 3 &&
    uint32_t(sl::DLSSMode::eUltraPerformance) == 4 && uint32_t(sl::DLSSMode::eDLAA) == 6);
static_assert(sl::kBufferTypeScalingInputColor == 3 && sl::kBufferTypeScalingOutputColor == 4 &&
    sl::kBufferTypeDepth == 0 && sl::kBufferTypeMotionVectors == 1 && sl::kBufferTypeExposure == 13 &&
    sl::kBufferTypeTransparencyHint == 27 && sl::kBufferTypeBiasCurrentColorHint == 29 &&
    sl::kBufferTypeColorBeforeTransparency == 40 && sl::kBufferTypeTransparencyLayer == 51 &&
    sl::ResourceLifecycle::eOnlyValidNow == 0 && uint32_t(sl::ResourceType::eTex2d) == 0);
static_assert(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 && D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 8);

// The constructors set the struct versions and every default the Rust builders copy, which no
// static assertion can see. A mismatch disables the SDK path rather than sending a stale layout.
bool records_match() noexcept
{
    const sl::Preferences preferences{};
    const sl::Constants constants{};
    const sl::DLSSOptions options{};
    const sl::DLSSOptimalSettings optimal{};
    const sl::FeatureRequirements requirements{};
    const sl::AdapterInfo adapter{};
    const sl::ViewportHandle viewport(0u);
    sl::Resource resource(sl::ResourceType::eTex2d, nullptr);
    const sl::ResourceTag tag(&resource, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow);
    return preferences.structVersion == 1 && constants.structVersion == 2 && options.structVersion == 3 &&
        optimal.structVersion == 1 && requirements.structVersion == 2 && adapter.structVersion == 1 &&
        viewport.structVersion == 1 && resource.structVersion == 1 && tag.structVersion == 1 &&
        uint64_t(preferences.flags) == ((1ull << 0) | (1ull << 3) | (1ull << 6)) &&
        preferences.logLevel == sl::LogLevel::eDefault && !preferences.showConsole &&
        constants.clipToLensClip.row[0].x == FLT_MAX && constants.orthographicProjection == sl::Boolean::eFalse &&
        constants.motionVectorsDilated == sl::Boolean::eFalse &&
        constants.motionVectorsJittered == sl::Boolean::eFalse &&
        constants.minRelativeLinearDepthObjectSeparation == 40.0f &&
        options.preExposure == 1.0f && options.exposureScale == 1.0f &&
        options.colorBuffersHDR == sl::Boolean::eTrue && options.indicatorInvertAxisX == sl::Boolean::eFalse &&
        options.indicatorInvertAxisY == sl::Boolean::eFalse && uint32_t(options.dlaaPreset) == 0 &&
        resource.type == sl::ResourceType::eTex2d && resource.state == UINT_MAX;
}

} // namespace
#endif

// Whether the pinned Streamline headers were available and their records match what Rust builds.
extern "C" uint32_t rsf_dlss_sdk_compiled()
{
#if RSF_HAVE_STREAMLINE
    static const bool matches = records_match();
    return matches;
#else
    return 0;
#endif
}

extern "C" uint32_t rsf_dlss_verify_runtime_signature(const wchar_t* path)
{
#if RSF_HAVE_STREAMLINE && RSF_HAVE_SIGNATURE_CHECK
    try {
        return sl::security::verifyEmbeddedSignature(path);
    } catch (...) {
        return 0;
    }
#else
    (void)path;
    return 0;
#endif
}
