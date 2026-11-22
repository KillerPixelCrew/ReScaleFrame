// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/input_dump.h>
// The native leaf binds the capture platform before the Rust metadata owner records the pass.
#ifdef __cplusplus
extern "C" {
#endif
void rsf_capture_sr_parameters(const rsf_frame_record* record, uint32_t backend,
    float x, float y, float exposure, uint32_t reset);
#ifdef __cplusplus
}
#endif
