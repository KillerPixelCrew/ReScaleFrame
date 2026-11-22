/* SPDX-License-Identifier: MIT */
#pragma once

#include <stdint.h>

typedef struct rsf_slhost_tag {
    void* resource;
    uint32_t state, kind, top, left, width, height;
} rsf_slhost_tag;

typedef struct rsf_slhost_reflex_sample {
    uint64_t id, simulation_start, simulation_end, submit_start, submit_end,
        present_start, present_end, driver_start, driver_end, queue_start, queue_end,
        gpu_start, gpu_end;
    uint32_t gpu_active, gpu_frame, cross_adapter_copy;
} rsf_slhost_reflex_sample;

typedef struct rsf_slhost_version {
    uint32_t ngx[3];
    uint32_t streamline[3];
} rsf_slhost_version;
