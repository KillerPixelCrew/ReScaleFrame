// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

/* Private adapters. Status-returning callbacks translate provider errors to
   rsf_backend_result. WSI calls preserve VkResult as returned by the provider. */
typedef struct rsf_fgvk_provider {
    rsf_backend_result (*create)(const rsf_fgvk_setup*, const rsf_fgvk_host_options*, uint32_t, void**, uint64_t*);
    rsf_backend_result (*enable)(void*, uint32_t, uint32_t);
    int32_t (*images)(void*, uint32_t*, uint64_t*);
    int32_t (*acquire)(void*, uint64_t, uint64_t, uint64_t, uint32_t*);
    rsf_backend_result (*image_layout)(void*, uint32_t*);
    rsf_backend_result (*max_generated)(void*, uint32_t*);
    rsf_backend_result (*prepare)(void*, void*, const rsf_fgvk_frame*);
    rsf_backend_result (*hudless)(void*, void*, const rsf_fgvk_frame*);
    void (*submitted)(void*, void* const*, uint32_t);
    int32_t (*present)(void*, const void*);
    int32_t (*present_source)(void*, const void*, const rsf_frame_record*);
    rsf_backend_result (*status)(void*, rsf_fgvk_status*);
    rsf_backend_result (*drain)(void*);
    rsf_backend_result (*destroy)(void*);
    rsf_backend_result (*latency_options)(void*, uint32_t, uint32_t);
    rsf_backend_result (*latency_sleep)(void*, uint32_t);
    rsf_backend_result (*latency_marker)(void*, uint32_t, uint32_t);
    rsf_backend_result (*latency_status)(void*, rsf_fgvk_latency_status*);
} rsf_fgvk_provider;

typedef struct rsf_fgvk_platform {
    const rsf_fgvk_provider* providers[6]; /* Indexed by the public FG backend ID. */
} rsf_fgvk_platform;

#ifdef __cplusplus
extern "C" {
#endif
rsf_backend_result rsf_core_fgvk_create(const rsf_fgvk_platform*, const rsf_fgvk_setup*, uint32_t backend,
    uint32_t count, const rsf_fgvk_host_options*, rsf_fgvk**, uint64_t*);
#ifdef __cplusplus
}
#endif
