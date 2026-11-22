// SPDX-License-Identifier: MIT
#pragma once
#include "native_dx12_vulkan_platform.h"
#include "native_sr_fsr4_vulkan.h"

#include "vulkan_fg_copy_abi.h"

typedef struct rsf_fsr4vk_platform {
    const rsf_dx12vk_platform* interop;
    int32_t (*open)(const rsf_srvk_setup*, void**);
    void (*close)(void*);
    int32_t (*bridge_create)(const rsf_fsr4_bridge_create*, rsf_fsr4_bridge**);
    int32_t (*bridge_plan)(rsf_fsr4_bridge*, uint32_t, uint32_t*, uint32_t*);
    int32_t (*bridge_info)(const rsf_fsr4_bridge*, rsf_fsr4_bridge_info*);
    int32_t (*create_fence)(void*, void**);
    int32_t (*create_image)(rsf_fsr4_bridge*, uint32_t, uint32_t, uint32_t, uint32_t, void**, void**);
    void (*release_image)(void*);
    int32_t (*capture_initialize)(void*, const rsf_srvk_setup*, uint64_t);
    void (*capture_begin)(const rsf_frame_record*);
    void (*capture_record)(void*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint32_t);
    void (*capture_parameters)(const rsf_srvk_frame*, uint32_t);
    int32_t (*capture_flush)(void*);
    void (*transition)(void*, void*, const struct rsf_vkfg_barrier*);
    void (*copy)(void*, void*, uint64_t, uint64_t, uint32_t, uint32_t);
    int32_t (*split)(const rsf_fsr4vk_handoff*, void*, uint64_t, uint64_t, uint64_t, uint64_t, void**);
    int32_t (*sharpening_valid)(const rsf_sharpening_options*);
    int32_t (*bridge_sharpness)(rsf_fsr4_bridge*, float);
    int32_t (*bridge_evaluate)(rsf_fsr4_bridge*, const rsf_fsr4_bridge_frame*);
    void (*sharpen)(void*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*, uint64_t);
    void (*retire)(void*, void*);
    void (*clear_sharpen)(void*);
    int32_t (*wait)(void*, uint64_t, uint64_t, uint64_t);
    int32_t (*idle)(void*);
    int32_t (*bridge_prepare_destroy)(rsf_fsr4_bridge*);
    int32_t (*bridge_destroy)(rsf_fsr4_bridge*);
} rsf_fsr4vk_platform;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_core_fsr4vk_create(const rsf_fsr4vk_platform*, const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
#ifdef __cplusplus
}
#endif
