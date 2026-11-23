// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_fg_resources.h"
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include "../../vulkan/streamline/sdk_sr_options.h"

typedef struct rsf_vkdlss_images {
    rsf_vkfg_image color, depth, motion, bias, output;
} rsf_vkdlss_images;

typedef struct rsf_vkdlss_tag {
    const rsf_vkfg_image* image;
    uint32_t kind, layout, width, height;
} rsf_vkdlss_tag;

typedef struct rsf_vkdlss_init {
    void* native;
    uint32_t width, height;
    const rsf_vkdlss_options* options;
} rsf_vkdlss_init;

typedef struct rsf_vkdlss_platform {
    int32_t (*open)(const rsf_srvk_setup*, void** native);
    void (*close)(void*);
    int32_t (*invoke)(const rsf_slvk_call*);
    rsf_slvk_callback initialize;
    int32_t (*prepare)(void*);
    uint32_t (*allocate)(void*, rsf_vkfg_image*, uint32_t width, uint32_t height, uint32_t format, uint32_t usage);
    void (*free_image)(void*, rsf_vkfg_image*);
    int32_t (*constants)(void*, uintptr_t token, const rsf_slvk_sdk_constants*);
    int32_t (*options)(void*, const rsf_vkdlss_options*);
    int32_t (*tag)(void*, uintptr_t token, void* commands, const rsf_vkdlss_tag* tags);
    int32_t (*evaluate)(void*, uintptr_t token, void* commands);
    void (*capture_parameters)(void*, const rsf_srvk_frame*, uint32_t reset);
    void (*capture_record)(void*, void* commands, const rsf_srvk_image*, const rsf_frame_record*,
                           const char* role, uint32_t reset);
    void (*sharpen)(void*, void* commands, const rsf_srvk_frame*, uint64_t output);
    int32_t (*release)(void*, uintptr_t, void*);
    void (*flush)(void*);
    void (*retire)(void*, void* commands);
    int32_t (*preset_valid)(uint32_t preset);
} rsf_vkdlss_platform;
