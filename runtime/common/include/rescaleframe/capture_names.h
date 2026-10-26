// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/input_dump.h>
#include <algorithm>
#include <string>
namespace rsf {
inline std::string capture_name(const char* role, uint32_t ordinal = 0)
{
    std::string name = role ? role : "image";
    if (name == "sr-linear-color") name = "sr_color";
    else if (name == "sr-output-linear") name = "sr_output";
    else if (name == "fg-hudless-color") name = "hudless_color";
    else if (name == "fg-motion-pixels" || name == "fg-motion-uv") name = "fg_motion";
    else if (name == "sr-motion-pixels" || name == "sr-motion-uv") name = "sr_motion";
    else if (name == "sr-game-motion-uv") name = "sr_game_motion";
    else std::replace(name.begin(), name.end(), '-', '_');
    if (ordinal) name += '_' + std::to_string(ordinal);
    return name;
}
inline rsf_dump_view capture_view(const std::string& role)
{
    if (role.find("depth") != std::string::npos) return RSF_DUMP_VIEW_DEPTH;
    if (role.find("alpha") != std::string::npos) return RSF_DUMP_VIEW_ALPHA;
    if (role.find("motion") != std::string::npos) return RSF_DUMP_VIEW_DENSE_MOTION;
    if (role.find("mask") != std::string::npos || role.find("exposure") != std::string::npos) return RSF_DUMP_VIEW_SCALAR;
    return RSF_DUMP_VIEW_HDR;
}
inline float capture_scale(const std::string& role)
{ return role.find("motion") == std::string::npos ? 1.0f : role.find("uv") != std::string::npos ? 8.0f : 1.0f / 32; }
inline void capture_sr_parameters(const rsf_frame_record& record, uint32_t backend, float x, float y, float exposure, uint32_t reset)
{
    if (!rsf_input_dump_wants(&record)) return;
    rsf_sr_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.jitter_x = record.camera.jitter_pixels[0]; frame.jitter_y = record.camera.jitter_pixels[1];
    frame.motion_scale_x = x; frame.motion_scale_y = y; frame.pre_exposure = exposure; frame.reset = reset;
    frame.view_space_to_meters = 1;
    rsf_input_dump_sr_frame(&frame, backend, 0);
}
}
