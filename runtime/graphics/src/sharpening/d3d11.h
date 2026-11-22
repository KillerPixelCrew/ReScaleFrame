/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <rescaleframe/sharpening_settings.h>

typedef struct rsf_sharpen11_frame {
    uint32_t depth_linear, depth_inverted, hdr;
    float near_plane, far_plane, exposure, motion_x, motion_y;
} rsf_sharpen11_frame;

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_sharpening_requested(const rsf_sharpening_options*);
/* Internal immediate-context pass. Inputs are output, motion, depth and exposure.
   The caller serializes access to the handle. Off succeeds with null resources. */
uint32_t rsf_core_sharpen11_run(void** owner, void* context, void* const* inputs,
    const rsf_sharpening_options*, const rsf_sharpen11_frame*);
uint64_t rsf_core_sharpen11_count(const void* owner);
void rsf_core_sharpen11_destroy(void* owner);
#ifdef __cplusplus
}
#endif
