// SPDX-License-Identifier: MIT
#pragma once
#include <sl.h>
#include <sl_dlss.h>
#include <cstddef>

struct rsf_streamline_sr_options {
    uint32_t mode, width, height, hdr, auto_exposure, alpha, preset;
};

inline constexpr uint32_t sr_max_tags = 9;
struct rsf_streamline_sr_tag {
    void* texture;
    uint32_t kind, state, size[2], has_extent;
};
struct rsf_streamline_sr_tags {
    uint32_t count, lifecycle;
    rsf_streamline_sr_tag entries[sr_max_tags];
};

static_assert(sizeof(rsf_streamline_sr_options) == 28);
static_assert(sizeof(rsf_streamline_sr_tag) == 32 && sizeof(rsf_streamline_sr_tags) == 296 &&
    offsetof(rsf_streamline_sr_tags, entries) == 8);
static_assert(sl::kBufferTypeScalingInputColor == 3 && sl::kBufferTypeScalingOutputColor == 4 &&
    sl::kBufferTypeDepth == 0 && sl::kBufferTypeMotionVectors == 1 && sl::kBufferTypeExposure == 13 &&
    sl::kBufferTypeTransparencyHint == 27 && sl::kBufferTypeBiasCurrentColorHint == 29 &&
    sl::kBufferTypeColorBeforeTransparency == 40 && sl::kBufferTypeTransparencyLayer == 51 &&
    sl::ResourceLifecycle::eOnlyValidNow == 0);

namespace rsf {
inline sl::DLSSOptions sr_options(const rsf_streamline_sr_options& input)
{
    sl::DLSSOptions options{};
    const auto preset = static_cast<sl::DLSSPreset>(input.preset);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = preset;
    options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    options.mode = static_cast<sl::DLSSMode>(input.mode);
    options.outputWidth = input.width;
    options.outputHeight = input.height;
    options.colorBuffersHDR = input.hdr ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    options.useAutoExposure = input.auto_exposure ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    options.alphaUpscalingEnabled = input.alpha ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    return options;
}


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
