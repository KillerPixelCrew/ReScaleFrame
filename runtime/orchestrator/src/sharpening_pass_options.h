/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/sharpening_settings.h>
#include "../../graphics/src/sharpening/options.h"

namespace rsf::sharpening {
inline Options options_from_request(const rsf_sharpening_options& request)
{
    Options options;
    options.mode = static_cast<Mode>(request.mode);
    options.sharpness = request.sharpness;
    options.mas = request.mas;
    options.motion_sharpness = request.motion_sharpness;
    options.motion_threshold = request.motion_threshold;
    options.motion_scale_limit = request.motion_scale_limit;
    options.contrast_enabled = request.contrast_enabled;
    options.contrast = request.contrast;
    options.depth_scale = request.depth_scale;
    options.depth_bias = request.depth_bias;
    options.clamp_output = request.clamp_output;
    options.debug = request.debug;
    return options;
}
}
