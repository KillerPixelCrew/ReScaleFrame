// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_fg_resources.h"
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>

struct rsf_dlssvk;

typedef struct rsf_vkdlss_images {
    rsf_vkfg_image color, depth, motion, bias, output;
} rsf_vkdlss_images;

typedef struct rsf_vkdlss_platform {
    int32_t (*open)(const rsf_srvk_setup*, void** native, uint32_t* width, uint32_t* height);
    void (*close)(void*);
    int32_t (*invoke)(const rsf_slvk_call*);
    uint32_t (*allocate)(void*, rsf_vkfg_image*, uint32_t width, uint32_t height, uint32_t format, uint32_t usage);
    void (*free_image)(void*, rsf_vkfg_image*);
    int32_t (*constants)(void*, uintptr_t token, const rsf_srvk_frame*, uint32_t reset);
    int32_t (*options)(void*, const rsf_srvk_frame*, uint32_t preset);
    void (*stage)(void*, void* commands, const rsf_srvk_image*, rsf_vkfg_image*, uint32_t bias);
    void (*initialize_output)(void*, void* commands, const rsf_vkfg_image*);
    int32_t (*tag)(void*, uintptr_t token, void* commands, const rsf_srvk_frame*, const rsf_vkdlss_images*);
    int32_t (*evaluate)(void*, uintptr_t token, void* commands);
    void (*capture)(void*, void* commands, const rsf_srvk_frame*, const rsf_vkfg_image*, uint32_t reset);
    void (*sharpen)(void*, void* commands, const rsf_srvk_frame*, uint64_t output);
    void (*publish)(void*, void* commands, const rsf_srvk_frame*, uint64_t output);
    int32_t (*release)(void*, uintptr_t, void*);
    void (*flush)(void*);
    void (*retire)(void*, void* commands);
} rsf_vkdlss_platform;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_core_vkdlss_create(const rsf_vkdlss_platform*, const rsf_srvk_setup*, struct rsf_dlssvk**);
int32_t rsf_core_vkdlss_plan(const struct rsf_dlssvk*, uint32_t*, uint32_t*);
int32_t rsf_core_vkdlss_evaluate(struct rsf_dlssvk*, void*, const rsf_srvk_frame*);
int32_t rsf_core_vkdlss_preset(struct rsf_dlssvk*, uint32_t);
int32_t rsf_core_vkdlss_destroy(struct rsf_dlssvk*);
void rsf_core_vkdlss_retire(struct rsf_dlssvk*, void*);
#ifdef __cplusplus
}
#endif
