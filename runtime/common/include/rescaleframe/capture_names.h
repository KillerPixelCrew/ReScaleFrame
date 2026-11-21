// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/input_dump.h>
namespace rsf {
void capture_sr_parameters(const rsf_frame_record& record, uint32_t backend, float x,
    float y, float exposure, uint32_t reset);
}
