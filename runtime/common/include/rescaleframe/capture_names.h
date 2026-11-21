// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/input_dump.h>
namespace rsf {
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
