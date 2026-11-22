/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

typedef struct rsf_fg12_readback_layout {
    uint64_t bytes, offset;
    uint32_t width, height, format, row_pitch;
} rsf_fg12_readback_layout;
