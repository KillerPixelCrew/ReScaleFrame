// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include "vulkan_fg_resources.h"

/* Private contract between the Rust sharpening owner and its native Vulkan
   leaves. Handles are Vulkan non-dispatchable handles; commands is the
   caller's VkCommandBuffer. Nothing here is exported from a DLL. */
typedef struct rsf_sharpenvk rsf_sharpenvk;

/* Per-recording uniform block and descriptor set. Mapped is host coherent. */
typedef struct rsf_sharpenvk_uniform {
    uint64_t pool, set, buffer, memory;
    void* mapped;
} rsf_sharpenvk_uniform;

typedef struct rsf_sharpenvk_layouts {
    uint64_t descriptor_set, pipeline;
} rsf_sharpenvk_layouts;

/* Facts for the shader constant block. Sizes are pixels. */
typedef struct rsf_sharpenvk_facts {
    uint32_t width, height, motion_width, motion_height, depth_width, depth_height;
    uint32_t depth_inverted, exposure_texture, effective_mode;
    float near_plane, far_plane, exposure, motion_x, motion_y;
} rsf_sharpenvk_facts;

/* One filter pass. Sampled views fill bindings 1 to 4: output colour,
   motion, depth and exposure. The output is GENERAL before and after. */
typedef struct rsf_sharpenvk_pass {
    uint64_t set, buffer, pipeline, pipeline_layout;
    uint64_t sampled[4];
    uint64_t scratch, scratch_view, output;
    uint32_t width, height, groups_x, groups_y, scratch_initialized, reserved;
} rsf_sharpenvk_pass;

typedef struct rsf_sharpenvk_platform {
    /* Scratch images use the shared Vulkan image leaves. */
    const rsf_vkfg_platform* images;
    int32_t (*open)(const rsf_srvk_setup*, void** device);
    void (*close)(void* device);
    void* (*image_user)(void* device);
    int32_t (*create_layouts)(void* device, rsf_sharpenvk_layouts*);
    void (*release_layouts)(void* device, rsf_sharpenvk_layouts*);
    /* Mode 1 to 3 selects the RCAS, depth RCAS or depth DAS SPIR-V. */
    int32_t (*create_pipeline)(void* device, uint64_t pipeline_layout, uint32_t mode, uint64_t* pipeline);
    void (*destroy_pipeline)(void* device, uint64_t pipeline);
    int32_t (*create_uniform)(void* device, uint64_t descriptor_layout, rsf_sharpenvk_uniform*);
    void (*release_uniform)(void* device, rsf_sharpenvk_uniform*);
    int32_t (*create_view)(void* device, uint64_t image, uint32_t format, uint64_t* view);
    /* Sanitizes request, or the defaults when it is null. Returns nonzero when
       the options require a filter dispatch. */
    uint32_t (*options)(const rsf_sharpening_options* request, rsf_sharpening_options* sanitized);
    void (*write_constants)(void* mapped, const rsf_sharpening_options*, const rsf_sharpenvk_facts*);
    void (*record)(void* device, void* commands, const rsf_sharpenvk_pass*);
    void (*log)(const rsf_srvk_frame*, const char* message);
} rsf_sharpenvk_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* The caller serializes every call on one owner. */
rsf_sharpenvk* rsf_core_sharpenvk_create(const rsf_sharpenvk_platform*);
/* Clears, then frees the owner. Every user must be drained. */
void rsf_core_sharpenvk_destroy(rsf_sharpenvk*);
/* Records the configured filter into commands. Returns nonzero when the
   output holds the requested result, including Off and Vendor modes. A
   refusal records nothing and leaves the output unchanged. */
uint32_t rsf_core_sharpenvk_record(rsf_sharpenvk*, const rsf_srvk_setup*, void* commands,
    const rsf_srvk_frame*, uint64_t output, uint32_t report_failure);
/* Call after GPU completion and reset of every recording on commands. */
void rsf_core_sharpenvk_retire(rsf_sharpenvk*, void* commands);
/* Releases the device, pipelines and slots. Every user must be drained. */
void rsf_core_sharpenvk_clear(rsf_sharpenvk*);
uint64_t rsf_core_sharpenvk_dispatches(const rsf_sharpenvk*);
#ifdef __cplusplus
}
#endif
