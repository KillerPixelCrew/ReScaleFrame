// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/input_dump_vulkan.h>
#include "../../../runtime/orchestrator/src/native_fg_fsr4_vulkan.h"

typedef struct rsf_godot_vk_capture_ops {
    int32_t (*create)(const rsf_input_dump_vk_setup*, rsf_input_dump_vk**);
    int32_t (*snapshot)(rsf_input_dump_vk*, const rsf_frame_record*, const rsf_srvk_image*, const char*);
    int32_t (*drain)(rsf_input_dump_vk*);
    int32_t (*destroy)(rsf_input_dump_vk*);
} rsf_godot_vk_capture_ops;

/* CMake supplies facts and references only for linked provider objects. */
typedef struct rsf_godot_vk_features {
    uint32_t sr, fsr4_sr, xess_fg;
    int32_t (*fsr4_probe)(const rsf_srvk_setup*, rsf_fsr4fg_info*);
    rsf_godot_vk_capture_ops capture;
} rsf_godot_vk_features;

#ifdef __cplusplus
extern "C" {
#endif
const rsf_godot_vk_features* rsf_godot_vk_native_features(void);
#ifdef __cplusplus
}
#endif
