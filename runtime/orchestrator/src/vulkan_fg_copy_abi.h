// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

struct rsf_vkfg_barrier {
    uint64_t image;
    uint32_t old_layout, new_layout, source_family, target_family;
    uint32_t source_access, target_access, source_stages, target_stages;
};

struct rsf_vkfg_image_copy {
    uint64_t source, target;
    uint32_t source_layout, target_layout, width, height;
};
