// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_dlss_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include "vulkan_fg_resources.h"
#include "../../vulkan/streamline/sdk_fg_options.h"

typedef struct rsf_dlgvk_sample {
    uint64_t semaphore, value;
    uint32_t actual_presents, max_generated, status, min_dimension, vsync, dynamic_mfg;
} rsf_dlgvk_sample;

typedef struct rsf_dlgvk_tag {
    const rsf_vkfg_image* image;
    uint32_t kind, lifetime, width, height;
} rsf_dlgvk_tag;

typedef struct rsf_dlgvk_sdk_call {
    void* native;
    int32_t result;
    rsf_dlgvk_sample sample;
    void* commands;
    const rsf_slvk_sdk_constants* camera;
    const rsf_frame_record* record;
    const rsf_dlgvk_tag* tags; /* Two tags for the leased recording. */
    const rsf_slvk_sdk_fg_options* options;
} rsf_dlgvk_sdk_call;

/* Streamline entry points at the start of the native owner. The initialize
   leaf resolves them; the other SDK leaves call them. Tokens are borrowed
   sl::FrameToken pointers and the command buffer is the leased recording. */
typedef struct rsf_dlgvk_sdk {
    int32_t (*set_constants)(const rsf_slvk_sdk_constants*, const void* token, const rsf_slvk_sdk_viewport*);
    int32_t (*set_tags)(const void* token, const rsf_slvk_sdk_viewport*, const rsf_slvk_sdk_tag*, uint32_t count,
                        void* commands);
    int32_t (*free_resources)(uint32_t feature, const rsf_slvk_sdk_viewport*);
    int32_t (*set_options)(const rsf_slvk_sdk_viewport*, const rsf_slvk_sdk_fg_options*);
    int32_t (*get_state)(const rsf_slvk_sdk_viewport*, rsf_slvk_sdk_fg_state*, const rsf_slvk_sdk_fg_options*);
} rsf_dlgvk_sdk;

typedef struct rsf_dlgvk_platform {
    const rsf_vkfg_platform* gpu;
    rsf_backend_result (*open)(const rsf_fgvk_setup*, void**);
    void (*close)(void*);
    rsf_backend_result (*prepare_chain)(void*, uint32_t* capture_enabled, void** gpu);
    rsf_backend_result (*start_capture)(void*);
    rsf_backend_result (*invoke)(const rsf_slvk_call*);
    rsf_slvk_callback initialize, configure, sample, constants, tag, release;
    void (*capture)(void*, void*, const rsf_srvk_image*, const rsf_frame_record*, uint32_t role, uint64_t group);
    void (*remember)(void*, const rsf_frame_record*, const rsf_dlssgvk_status*, uint32_t configured);
    void (*capture_group)(void*, uint64_t, uint32_t complete);
    void (*flush)(void*);
    rsf_backend_result (*detach_capture)(void*);
    uint64_t (*timeout_epoch)(void);
} rsf_dlgvk_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* Production Streamline table in native_fg_dlss_vulkan.c; its leaves are Rust
   (vulkan_dlss_fg/leaves.rs). */
extern const rsf_dlgvk_platform rsf_dlgvk_leaves;
void rsf_core_dlgvk_report_api_error(int32_t error);
rsf_backend_result rsf_core_dlgvk_create(const rsf_dlgvk_platform*, const rsf_fgvk_setup*, rsf_dlssgvk**, uint64_t*);
int32_t rsf_core_dlgvk_images(rsf_dlssgvk*, uint32_t*, uint64_t*);
int32_t rsf_core_dlgvk_acquire(rsf_dlssgvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
rsf_backend_result rsf_core_dlgvk_image_layout(const rsf_dlssgvk*, uint32_t*);
rsf_backend_result rsf_core_dlgvk_enable(rsf_dlssgvk*, uint32_t, uint32_t);
rsf_backend_result rsf_core_dlgvk_record(rsf_dlssgvk*, void*, const rsf_fgvk_frame*, uint32_t hudless);
void rsf_core_dlgvk_submitted(rsf_dlssgvk*, void* const*, uint32_t);
int32_t rsf_core_dlgvk_present(rsf_dlssgvk*, const void*, const rsf_frame_record*, uint32_t explicit_source);
rsf_backend_result rsf_core_dlgvk_status(rsf_dlssgvk*, rsf_dlssgvk_status*);
rsf_backend_result rsf_core_dlgvk_drain(rsf_dlssgvk*);
rsf_backend_result rsf_core_dlgvk_destroy(rsf_dlssgvk*);
#ifdef __cplusplus
}
#endif
