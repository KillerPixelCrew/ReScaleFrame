// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_dlss_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include "vulkan_fg_resources.h"

struct rsf_dlgvk_sample {
    uint64_t semaphore, value;
    uint32_t actual_presents, max_generated, status, min_dimension, vsync, dynamic_mfg;
};

struct rsf_dlgvk_sdk_call {
    void* native;
    int32_t result;
    uint32_t count; // Generated frames (zero is Off) for configure, staged images for tag.
    rsf_dlgvk_sample sample;
    void* commands;
    const rsf_fgvk_frame* input;
    const rsf_frame_record* record;
    const rsf_vkfg_image* images;
    uint32_t backbuffers;
};

struct rsf_dlgvk_swapchain {
    uint32_t width, height, format, layers, has_surface;
};

struct rsf_dlgvk_chain_support {
    uint32_t usage, capture_enabled;
};

struct rsf_dlgvk_platform {
    const rsf_vkfg_platform* gpu;
    void (*describe)(const void*, rsf_dlgvk_swapchain*);
    rsf_backend_result (*open)(const rsf_fgvk_setup*, void**);
    void (*close)(void*);
    rsf_backend_result (*prepare_chain)(void*, rsf_dlgvk_chain_support*, void** gpu);
    int32_t (*create_chain)(void*, uint32_t usage, uint64_t*);
    void (*destroy_chain)(void*, uint64_t);
    rsf_backend_result (*start_capture)(void*);
    int32_t (*images)(void*, uint64_t chain, uint32_t*, uint64_t*);
    int32_t (*acquire)(void*, uint64_t chain, uint64_t, uint64_t, uint64_t, uint32_t*);
    uint32_t (*valid_present)(uint64_t chain, const void*);
    int32_t (*present)(void*, const void*);
    rsf_backend_result (*invoke)(const rsf_slvk_call*);
    rsf_slvk_callback initialize, configure, sample, constants, tag, release;
    void (*stage)(void*, void*, const rsf_srvk_image*, const rsf_vkfg_image*);
    void (*capture)(void*, void*, const rsf_srvk_image*, const rsf_frame_record*, uint32_t role, uint64_t group);
    void (*remember)(void*, const rsf_frame_record*, const rsf_dlssgvk_status*, uint32_t configured);
    void (*capture_group)(void*, uint64_t, uint32_t complete);
    void (*flush)(void*);
    rsf_backend_result (*detach_capture)(void*);
    uint64_t (*timeout_epoch)();
    int32_t (*idle)(void*);
    int32_t (*wait_inputs)(void*, uint64_t, uint64_t);
};

extern "C" {
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
}
