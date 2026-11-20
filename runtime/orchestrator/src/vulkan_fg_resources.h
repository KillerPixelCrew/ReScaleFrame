// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_fg_frames.h"

typedef struct rsf_vkfg_image {
    uint64_t image, memory, view;
    uint32_t width, height, format, initialized;
} rsf_vkfg_image;

typedef struct rsf_vkfg_fences {
    uint64_t source, consumer;
} rsf_vkfg_fences;

typedef struct rsf_vkfg_platform {
    uint32_t (*local_memory_types)(void*);
    int32_t (*create_image)(void*, uint32_t width, uint32_t height, uint32_t format, uint64_t*);
    void (*requirements)(void*, uint64_t image, uint64_t* size, uint32_t* types);
    int32_t (*allocate)(void*, uint64_t size, uint32_t type, uint64_t*);
    int32_t (*bind)(void*, uint64_t image, uint64_t memory);
    int32_t (*create_view)(void*, uint64_t image, uint32_t format, uint64_t*);
    void (*destroy_view)(void*, uint64_t);
    void (*destroy_image)(void*, uint64_t);
    void (*free_memory)(void*, uint64_t);
    int32_t (*create_fence)(void*, uint64_t*);
    void (*destroy_fence)(void*, uint64_t);
    int32_t (*wait_fence)(void*, uint64_t);
    int32_t (*reset_fence)(void*, uint64_t);
    int32_t (*submit_fence)(void*, uint64_t);
} rsf_vkfg_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* The native owner serializes slot access and drains it before releasing a
   committed image or fence. Failed replacement leaves the old image intact. */
uint32_t rsf_core_vkfg_ensure_image(const rsf_vkfg_platform*, void*, rsf_vkfg_image*,
    uint32_t width, uint32_t height, uint32_t format, uint32_t with_view);
void rsf_core_vkfg_release_image(const rsf_vkfg_platform*, void*, rsf_vkfg_image*);
int32_t rsf_core_vkfg_prepare_fences(const rsf_vkfg_platform*, void*, rsf_fgvk_frame_slot*, rsf_vkfg_fences*);
void rsf_core_vkfg_fence_submission(const rsf_vkfg_platform*, void*, rsf_fgvk_frame_slot*,
    const rsf_vkfg_fences*, uint32_t consumer);
void rsf_core_vkfg_release_fences(const rsf_vkfg_platform*, void*, rsf_vkfg_fences*);
#ifdef __cplusplus
}
#endif
