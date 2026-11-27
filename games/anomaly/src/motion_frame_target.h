// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>

typedef struct rsf_anomaly_motion_frame_target {
    void* texture;
    uint32_t width, height, frame;
    bool written, main;
} rsf_anomaly_motion_frame_target;
