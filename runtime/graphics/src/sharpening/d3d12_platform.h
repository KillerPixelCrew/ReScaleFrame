/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/sharpening_settings.h>

#ifdef __cplusplus
extern "C" {
#endif
/* The caller retires a slot before reuse and drains all slots before destruction.
   Output enters and leaves in UAV state. Optional inputs remain shader-readable. */
void* rsf_sharpen12_host_create(void);
void rsf_sharpen12_host_destroy(void*);
int32_t rsf_sharpen12_host_run(void* owner, void* commands, void* output, void* motion,
    void* depth, void* exposure, const rsf_sharpening_options*, uint32_t hdr,
    uint32_t depth_inverted, float near_plane, float far_plane, uint32_t slot);
#ifdef __cplusplus
}
#endif
