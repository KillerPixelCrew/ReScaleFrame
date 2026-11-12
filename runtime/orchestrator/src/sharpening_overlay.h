// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/overlay.h>
#include <rescaleframe/sharpening_settings.h>
namespace rsf::sharpening_overlay {
inline void fill(rsf_overlay_stats& s,const rsf_sharpening_options& o) {
    s.sharpening_available=1;
    s.sharpening_mode=o.mode;
    s.sharpening_sharpness=o.sharpness;
    s.sharpening_mas=o.mas;
    s.sharpening_motion_sharpness=o.motion_sharpness;
    s.sharpening_motion_threshold=o.motion_threshold;
    s.sharpening_motion_scale_limit=o.motion_scale_limit;
    s.sharpening_contrast_enabled=o.contrast_enabled;
    s.sharpening_contrast=o.contrast;
    s.sharpening_depth_scale=o.depth_scale;
    s.sharpening_depth_bias=o.depth_bias;
    s.sharpening_clamp_output=o.clamp_output;
    s.sharpening_debug=o.debug;
}
inline rsf_sharpening_options requested(const rsf_overlay_intent& i) {
    auto o=rsf_sharpening_defaults();
    o.mode=i.sharpening_mode;
    o.sharpness=i.sharpening_sharpness;
    o.mas=i.sharpening_mas;
    o.motion_sharpness=i.sharpening_motion_sharpness;
    o.motion_threshold=i.sharpening_motion_threshold;
    o.motion_scale_limit=i.sharpening_motion_scale_limit;
    o.contrast_enabled=i.sharpening_contrast_enabled;
    o.contrast=i.sharpening_contrast;
    o.depth_scale=i.sharpening_depth_scale;
    o.depth_bias=i.sharpening_depth_bias;
    o.clamp_output=i.sharpening_clamp_output;
    o.debug=i.sharpening_debug;
    return o;
}
}
