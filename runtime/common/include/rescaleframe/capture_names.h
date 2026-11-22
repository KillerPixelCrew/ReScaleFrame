// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/input_dump.h>
// The native leaf binds the capture platform before the Rust metadata owner records the pass.
extern "C" void rsf_capture_sr_parameters(const rsf_frame_record* record, uint32_t backend,
    float x, float y, float exposure, uint32_t reset);

namespace rsf {
inline void capture_sr_parameters(const rsf_frame_record& record, uint32_t backend, float x,
    float y, float exposure, uint32_t reset)
{
    rsf_capture_sr_parameters(&record, backend, x, y, exposure, reset);
}
}
