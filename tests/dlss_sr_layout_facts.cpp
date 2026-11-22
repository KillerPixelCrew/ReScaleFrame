// SPDX-License-Identifier: MIT
// Checks the Streamline records and constants that Rust builds against the vendor declarations.
// sl.h and sl_security.h are C++ only (namespaces, constructors, virtual members), so they cannot
// be measured from C or Rust. The static assertions fix the layouts, GUIDs and enum values at
// build time. The two exported functions report what the SDK constructors and enums actually
// hold; runtime/orchestrator/src/streamline_sdk/layout_fixture.rs holds the expectations and
// compares them, so a drifting Rust default or literal fails the dlss_sr_layout test.

#include <windows.h>

#include <d3d12.h>

#include <sl.h>
#include <sl_consts.h>
#include <sl_dlss.h>
#include "../runtime/vulkan/streamline/sdk_records_checks.h"

#include <cfloat>
#include <climits>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace {
#if defined(__clang__)
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Winvalid-offsetof"
#elif defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Winvalid-offsetof"
#endif

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

// The record layouts are in sdk_records_checks.h; these are the members it does not reach.
RSF_SL_AT(DLSSOptimalSettings, rsf_vkdlss_optimal, optimalSharpness, sharpness);
RSF_SL_AT(DLSSOptimalSettings, rsf_vkdlss_optimal, renderWidthMin, min_width);
RSF_SL_AT(DLSSOptimalSettings, rsf_vkdlss_optimal, renderHeightMin, min_height);
RSF_SL_AT(DLSSOptimalSettings, rsf_vkdlss_optimal, renderWidthMax, max_width);
RSF_SL_AT(DLSSOptimalSettings, rsf_vkdlss_optimal, renderHeightMax, max_height);
static_assert(sizeof(sl::DLSSPreset) == sizeof(uint32_t) &&
    offsetof(sl::DLSSOptions, qualityPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 4 &&
    offsetof(sl::DLSSOptions, balancedPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 8 &&
    offsetof(sl::DLSSOptions, performancePreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 12 &&
    offsetof(sl::DLSSOptions, ultraPerformancePreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 16 &&
    offsetof(sl::DLSSOptions, ultraQualityPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 20);

#undef RSF_SL_GUID
#undef RSF_SL_AT
#if defined(__clang__)
#pragma clang diagnostic pop
#elif defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

// The enum and constant values Rust carries as literals are compared at run time by
// rsf_test_dlss_sr_values below.
static_assert(sizeof(wchar_t) == sizeof(uint16_t));

template <class T>
size_t copy_object(const T& object, void* out, size_t capacity)
{
    static_assert(__is_trivially_copyable(T));
    if (out && capacity >= sizeof(T)) std::memcpy(out, &object, sizeof(T));
    return sizeof(T);
}

} // namespace

// The bytes of a default-constructed SDK object of the given kind, which Rust compares field by
// field. A null or short buffer only reports the size.
extern "C" size_t rsf_test_dlss_sr_default(uint32_t kind, void* out, size_t capacity)
{
    switch (kind) {
    case 0: return copy_object(sl::Preferences{}, out, capacity);
    case 1: return copy_object(sl::Constants{}, out, capacity);
    case 2: return copy_object(sl::DLSSOptions{}, out, capacity);
    case 3: return copy_object(sl::DLSSOptimalSettings{}, out, capacity);
    case 4: return copy_object(sl::FeatureRequirements{}, out, capacity);
    case 5: return copy_object(sl::AdapterInfo{}, out, capacity);
    case 6: return copy_object(sl::ViewportHandle(0u), out, capacity);
    case 7: return copy_object(sl::Resource(sl::ResourceType::eTex2d, nullptr), out, capacity);
    case 8:
        return copy_object(sl::ResourceTag(nullptr, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow),
            out, capacity);
    default: return 0;
    }
}

// The SDK values the Rust SR code carries as literals, in the order the Rust expectations list them.
extern "C" size_t rsf_test_dlss_sr_values(uint64_t* out, size_t capacity)
{
    const uint64_t values[] = {
        sl::kSDKVersion,
        sl::kFeatureDLSS,
        sl::kFeatureCommon,
        uint64_t(sl::PreferenceFlags::eUseManualHooking),
        uint64_t(sl::PreferenceFlags::eUseFrameBasedResourceTagging),
        uint64_t(sl::PreferenceFlags::eDisableCLStateTracking) | uint64_t(sl::PreferenceFlags::eAllowOTA) |
            uint64_t(sl::PreferenceFlags::eLoadDownloadedPlugins),
        uint64_t(sl::EngineType::eCustom),
        uint64_t(sl::EngineType::eUnreal),
        uint64_t(sl::EngineType::eUnity),
        uint64_t(sl::RenderAPI::eD3D11),
        uint64_t(sl::RenderAPI::eD3D12),
        uint64_t(sl::LogLevel::eDefault),
        uint64_t(sl::LogType::eWarn),
        uint64_t(sl::LogType::eError),
        uint64_t(uint32_t(sl::Result::eOk)),
        uint64_t(sl::Result::eErrorDriverOutOfDate),
        uint64_t(sl::Result::eErrorOSOutOfDate),
        uint64_t(sl::Result::eErrorNoSupportedAdapterFound),
        uint64_t(sl::DLSSMode::eDLAA),
        uint64_t(sl::DLSSMode::eMaxQuality),
        uint64_t(sl::DLSSMode::eBalanced),
        uint64_t(sl::DLSSMode::eMaxPerformance),
        uint64_t(sl::DLSSMode::eUltraPerformance),
        uint64_t(sl::DLSSMode::eOff),
        sl::kBufferTypeScalingInputColor,
        sl::kBufferTypeScalingOutputColor,
        sl::kBufferTypeDepth,
        sl::kBufferTypeMotionVectors,
        sl::kBufferTypeExposure,
        sl::kBufferTypeTransparencyHint,
        sl::kBufferTypeBiasCurrentColorHint,
        sl::kBufferTypeColorBeforeTransparency,
        sl::kBufferTypeTransparencyLayer,
        uint64_t(sl::ResourceLifecycle::eOnlyValidNow),
        uint64_t(sl::ResourceType::eTex2d),
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        uint64_t(sl::Boolean::eFalse),
        uint64_t(sl::Boolean::eTrue),
    };
    const size_t count = sizeof(values) / sizeof(values[0]);
    if (out && capacity >= count) std::memcpy(out, values, sizeof(values));
    return count;
}