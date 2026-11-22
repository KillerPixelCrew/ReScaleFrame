// SPDX-License-Identifier: MIT
// Keep the native diagnostic callback guard and receipt formatting.
#include "depth_domains_platform.h"
#include <cstdio>

namespace rsf::fallout4 {
namespace {
// The text the former refuse() wrote; Rust decides the stage and its once-per-stage receipt.
void refusal(NativeDepthReport report, const char* stage, const NativeDepthSnapshot* snapshot,
    const NativeDepthMathFailure* detail, uint32_t math) noexcept {
    if (!report || !stage || !snapshot) return;
    const auto& s = *snapshot;
    try {
        char text[800]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 depth stage=%s source=%llu submission=%llu native=%llu mode=%u range=%g,%g/%g,%g raster=%g,%g,%g,%g shadow=%g,%g,%g,%g dirty=%u selectors=%u,%u,%u descriptor=%ux%u descriptor_read=%u physical=%u/%u,%u ratio=%g,%g default=%u dynamic=%u",
            stage,static_cast<unsigned long long>(s.identity.source),
            static_cast<unsigned long long>(s.identity.submission),static_cast<unsigned long long>(s.native_frame),
            unsigned(s.mode),double(s.intervals[0]),double(s.intervals[1]),double(s.intervals[2]),double(s.intervals[3]),
            double(s.actual_raster[0]),double(s.actual_raster[1]),double(s.actual_raster[2]),double(s.actual_raster[3]),
            double(s.shadow_raster[0]),double(s.shadow_raster[1]),double(s.shadow_raster[2]),double(s.shadow_raster[3]),
            s.shadow_dirty,s.colour_selector,s.depth_selector,s.cube_selector,s.descriptor_width,s.descriptor_height,
            unsigned(s.descriptor_read),
            s.colour_slot,s.bound_colour_slot,s.bound_depth_slot,
            double(s.ratio[0]),double(s.ratio[1]),unsigned(s.default_viewport),unsigned(s.dynamic_viewport));
        report(text);
        if (detail && math) {
            char math_text[520]{};
            if (math == 1) {
                std::snprintf(math_text,sizeof(math_text),
                    "Fallout4 depth math stage=%s source=%llu native=%llu mismatch_index=%u actual=%.9g expected=%.9g relative_error=%.9g tolerance=0.0005",
                    stage,static_cast<unsigned long long>(s.identity.source),static_cast<unsigned long long>(s.native_frame),
                    detail->index,double(detail->actual),double(detail->expected),double(detail->relative_error));
            } else {
                std::snprintf(math_text,sizeof(math_text),
                    "Fallout4 depth math stage=%s source=%llu native=%llu W=%.9g,%.9g,%.9g,%.9g minimum=%.12g range=%.9g near_origin=%.9g,%.9g,%.9g world_origin=%.9g,%.9g,%.9g",
                    stage,static_cast<unsigned long long>(s.identity.source),static_cast<unsigned long long>(s.native_frame),
                    detail->w[0],detail->w[1],detail->w[2],detail->w[3],detail->minimum_w,detail->range,
                    double(s.near_camera.origin[0]),double(s.near_camera.origin[1]),double(s.near_camera.origin[2]),
                    double(s.world_camera.origin[0]),double(s.world_camera.origin[1]),double(s.world_camera.origin[2]));
            }
            report(math_text);
        }
    } catch (...) {}
}
// The text the former lease check wrote; Rust keeps its budget of sixteen.
void lease(NativeDepthReport report, const NativeDepthSnapshot* snapshot, const NativeDepthSnapshot* current,
    uint32_t reached, uint32_t passed, uint32_t stage, uint32_t changed) noexcept {
    if (!report || !snapshot || !current) return;
    try {
        char text[600]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 depth lease source=%llu native=%llu/%llu reached=0x%x passed=0x%x sample_stage=%u changed=0x%x mode=%u/%u default=%u/%u dynamic=%u/%u jitter=%g,%g/%g,%g ratio=%g,%g/%g,%g",
            static_cast<unsigned long long>(snapshot->identity.source),static_cast<unsigned long long>(snapshot->native_frame),
            static_cast<unsigned long long>(current->native_frame),reached,passed,stage,changed,
            unsigned(snapshot->mode),unsigned(current->mode),unsigned(snapshot->default_viewport),unsigned(current->default_viewport),
            unsigned(snapshot->dynamic_viewport),unsigned(current->dynamic_viewport),double(snapshot->jitter[0]),double(snapshot->jitter[1]),
            double(current->jitter[0]),double(current->jitter[1]),double(snapshot->ratio[0]),double(snapshot->ratio[1]),
            double(current->ratio[0]),double(current->ratio[1]));
        report(text);
    } catch (...) {}
}

constexpr NativeDepthLeaves leaves{refusal, lease};
}
const NativeDepthLeaves* native_depth_leaves() noexcept { return &leaves; }
}
