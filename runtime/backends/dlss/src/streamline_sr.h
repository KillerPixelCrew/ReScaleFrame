// SPDX-License-Identifier: MIT
#pragma once
#include "streamline_sr_payload.h"
#include <sl.h>
#include <sl_dlss.h>
#include <cstddef>
#include <cstring>
#include <type_traits>

inline constexpr uint32_t sr_max_tags = 9;
struct rsf_streamline_sr_tag {
    void* texture;
    uint32_t kind, state, size[2], has_extent;
};
struct rsf_streamline_sr_tags {
    uint32_t count, lifecycle;
    rsf_streamline_sr_tag entries[sr_max_tags];
};

static_assert(sizeof(rsf_streamline_sr_tag) == 32 && sizeof(rsf_streamline_sr_tags) == 296 &&
    offsetof(rsf_streamline_sr_tags, entries) == 8);
static_assert(sl::kBufferTypeScalingInputColor == 3 && sl::kBufferTypeScalingOutputColor == 4 &&
    sl::kBufferTypeDepth == 0 && sl::kBufferTypeMotionVectors == 1 && sl::kBufferTypeExposure == 13 &&
    sl::kBufferTypeTransparencyHint == 27 && sl::kBufferTypeBiasCurrentColorHint == 29 &&
    sl::kBufferTypeColorBeforeTransparency == 40 && sl::kBufferTypeTransparencyLayer == 51 &&
    sl::ResourceLifecycle::eOnlyValidNow == 0);

namespace rsf {
// The SDK records inherit a versioned header. Copy only the proven member
// ranges and leave their native constructors and untouched defaults in charge.
#if defined(__clang__)
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Winvalid-offsetof"
#elif defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Winvalid-offsetof"
#endif

#define RSF_SR_FIELD(Sdk, sdk_first, sdk_field, Wire, wire_first, wire_field)                     \
    static_assert(offsetof(Sdk, sdk_field) - offsetof(Sdk, sdk_first) ==                          \
        offsetof(Wire, wire_field) - offsetof(Wire, wire_first));                                 \
    static_assert(sizeof(decltype(Sdk::sdk_field)) == sizeof(decltype(Wire::wire_field)))

static_assert(sizeof(float) == 4 && std::is_trivially_copyable_v<sl::float4x4> &&
    std::is_trivially_copyable_v<sl::float2> && std::is_trivially_copyable_v<sl::float3> &&
    static_cast<uint8_t>(sl::Boolean::eFalse) == 0 && static_cast<uint8_t>(sl::Boolean::eTrue) == 1);
static_assert(offsetof(sl::float4x4, row) == 0 &&
    offsetof(sl::float4, x) == 0 && offsetof(sl::float4, y) == 4 &&
    offsetof(sl::float4, z) == 8 && offsetof(sl::float4, w) == 12 &&
    offsetof(sl::float3, x) == 0 && offsetof(sl::float3, y) == 4 && offsetof(sl::float3, z) == 8 &&
    offsetof(sl::float2, x) == 0 && offsetof(sl::float2, y) == 4);

RSF_SR_FIELD(sl::Constants, cameraViewToClip, cameraViewToClip, rsf_sl_sr_constants, view_to_clip, view_to_clip);
RSF_SR_FIELD(sl::Constants, cameraViewToClip, clipToCameraView, rsf_sl_sr_constants, view_to_clip, clip_to_view);
#define RSF_SR_CONSTANT(sdk, wire) \
    RSF_SR_FIELD(sl::Constants, clipToPrevClip, sdk, rsf_sl_sr_constants, clip_to_previous, wire)
RSF_SR_CONSTANT(clipToPrevClip, clip_to_previous);
RSF_SR_CONSTANT(prevClipToClip, previous_to_clip);
RSF_SR_CONSTANT(jitterOffset, jitter);
RSF_SR_CONSTANT(mvecScale, motion_scale);
RSF_SR_CONSTANT(cameraPinholeOffset, pinhole);
RSF_SR_CONSTANT(cameraPos, position);
RSF_SR_CONSTANT(cameraUp, up);
RSF_SR_CONSTANT(cameraRight, right);
RSF_SR_CONSTANT(cameraFwd, forward);
RSF_SR_CONSTANT(cameraNear, near_plane);
RSF_SR_CONSTANT(cameraFar, far_plane);
RSF_SR_CONSTANT(cameraFOV, fov);
RSF_SR_CONSTANT(cameraAspectRatio, aspect);
RSF_SR_CONSTANT(motionVectorsInvalidValue, invalid_motion);
RSF_SR_CONSTANT(depthInverted, depth_inverted);
RSF_SR_CONSTANT(cameraMotionIncluded, camera_motion);
RSF_SR_CONSTANT(motionVectors3D, motion_3d);
RSF_SR_CONSTANT(reset, reset);
#undef RSF_SR_CONSTANT

inline sl::Constants sr_constants(const rsf_sl_sr_constants& input)
{
    sl::Constants constants{};
    auto* bytes = reinterpret_cast<unsigned char*>(&constants);
    std::memcpy(bytes + offsetof(sl::Constants, cameraViewToClip), &input,
        offsetof(rsf_sl_sr_constants, clip_to_previous));
    const auto* source = reinterpret_cast<const unsigned char*>(&input);
    constexpr auto tail = offsetof(rsf_sl_sr_constants, clip_to_previous);
    std::memcpy(bytes + offsetof(sl::Constants, clipToPrevClip), source + tail, sizeof(input) - tail);
    static_assert(offsetof(rsf_sl_sr_constants, reset) + 1 == sizeof(input));
    return constants;
}

RSF_SR_FIELD(sl::DLSSOptions, mode, mode, rsf_sl_sr_dimensions, mode, mode);
RSF_SR_FIELD(sl::DLSSOptions, mode, outputWidth, rsf_sl_sr_dimensions, mode, width);
RSF_SR_FIELD(sl::DLSSOptions, mode, outputHeight, rsf_sl_sr_dimensions, mode, height);
static_assert(sizeof(rsf_sl_sr_dimensions) == 12);
static_assert(sizeof(sl::DLSSPreset) == sizeof(uint32_t));
static_assert(offsetof(sl::DLSSOptions, qualityPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 4 &&
    offsetof(sl::DLSSOptions, balancedPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 8 &&
    offsetof(sl::DLSSOptions, performancePreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 12 &&
    offsetof(sl::DLSSOptions, ultraPerformancePreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 16 &&
    offsetof(sl::DLSSOptions, ultraQualityPreset) == offsetof(sl::DLSSOptions, dlaaPreset) + 20);

inline sl::DLSSOptions sr_plan_options(const rsf_sl_sr_dimensions& dimensions)
{
    sl::DLSSOptions options{};
    auto* bytes = reinterpret_cast<unsigned char*>(&options);
    std::memcpy(bytes + offsetof(sl::DLSSOptions, mode), &dimensions, sizeof(dimensions));
    return options;
}

inline sl::DLSSOptions sr_options(const rsf_sl_sr_options& input)
{
    auto options = sr_plan_options(input.dimensions);
    auto* bytes = reinterpret_cast<unsigned char*>(&options);
    std::memcpy(bytes + offsetof(sl::DLSSOptions, dlaaPreset), input.presets, sizeof(input.presets));
    options.colorBuffersHDR = input.hdr ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    options.useAutoExposure = input.auto_exposure ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    options.alphaUpscalingEnabled = input.alpha ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    return options;
}

#define RSF_SR_OPTIMAL(sdk, wire) \
    RSF_SR_FIELD(sl::DLSSOptimalSettings, optimalRenderWidth, sdk, rsf_sl_sr_optimal, width, wire)
RSF_SR_OPTIMAL(optimalRenderWidth, width);
RSF_SR_OPTIMAL(optimalRenderHeight, height);
RSF_SR_OPTIMAL(optimalSharpness, sharpness);
RSF_SR_OPTIMAL(renderWidthMin, width_min);
RSF_SR_OPTIMAL(renderHeightMin, height_min);
RSF_SR_OPTIMAL(renderWidthMax, width_max);
RSF_SR_OPTIMAL(renderHeightMax, height_max);
#undef RSF_SR_OPTIMAL
#undef RSF_SR_FIELD

inline sl::Result sr_optimal(PFun_slDLSSGetOptimalSettings* plan, const rsf_sl_sr_dimensions& dimensions,
    rsf_sl_sr_optimal& out)
{
    sl::DLSSOptimalSettings settings{};
    const auto result = plan(sr_plan_options(dimensions), settings);
    if (result == sl::Result::eOk) {
        const auto* bytes = reinterpret_cast<const unsigned char*>(&settings);
        std::memcpy(&out, bytes + offsetof(sl::DLSSOptimalSettings, optimalRenderWidth), sizeof(out));
    }
    return result;
}

#if defined(__clang__)
#pragma clang diagnostic pop
#elif defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

inline sl::Result set_sr_tags(PFun_slSetTagForFrame* set_tags, const sl::FrameToken& token,
    const sl::ViewportHandle& viewport, const rsf_streamline_sr_tags& input, sl::CommandBuffer* commands)
{
    sl::Resource resources[sr_max_tags]{};
    sl::ResourceTag tags[sr_max_tags]{};
    for (uint32_t i = 0; i < input.count; ++i) {
        const auto& entry = input.entries[i];
        const sl::Extent extent{0, 0, entry.size[0], entry.size[1]};
        resources[i] = sl::Resource{sl::ResourceType::eTex2d, entry.texture, nullptr, nullptr, entry.state};
        tags[i] = sl::ResourceTag{&resources[i], static_cast<sl::BufferType>(entry.kind),
            static_cast<sl::ResourceLifecycle>(input.lifecycle), entry.has_extent ? &extent : nullptr};
    }
    return set_tags(token, viewport, tags, input.count, commands);
}
}
