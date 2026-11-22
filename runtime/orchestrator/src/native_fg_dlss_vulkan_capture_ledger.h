// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <rescaleframe/native_fg_dlss_vulkan.h>
#include "../../vulkan/streamline/present-capture.h"

typedef struct rsf_dlss_capture rsf_dlss_capture;

typedef struct rsf_dlss_capture_source {
    rsf_frame_record record;
    rsf_dlssgvk_status sdk;
    uint32_t generated_count, configured;
} rsf_dlss_capture_source;

typedef struct rsf_dlss_capture_target {
    uint64_t chain;
    uint32_t index, selected, usage, layers, format, width, height;
} rsf_dlss_capture_target;

/* Native leaves of the Rust present capture owner. Vulkan results are
   returned unchanged. Handles cross as opaque values, and the packet and
   source are borrowed for one call. */
typedef struct rsf_dlss_capture_platform {
    /* Writes a null native handle and succeeds while capture is disabled. */
    rsf_backend_result (*open)(const rsf_fgvk_setup*, void** native);
    void (*close)(void*);
    /* A null callback clears the registration for the same device and user. */
    int32_t (*set_callback)(void*, rsf_sl_present_capture_callback, void* user);
    int32_t (*create_pool)(void*, uint32_t queue_family, uint64_t*);
    void (*destroy_pool)(void*, uint64_t);
    int32_t (*allocate)(void*, uint64_t pool, void** commands);
    int32_t (*reset_pool)(void*, uint64_t);
    int32_t (*begin)(void*, void* commands);
    int32_t (*end)(void*, void* commands);
    int32_t (*create_fence)(void*, uint64_t*);
    void (*destroy_fence)(void*, uint64_t);
    int32_t (*reset_fence)(void*, uint64_t);
    int32_t (*wait_fence)(void*, uint64_t, uint64_t timeout);
    void (*complete_readback)(void*, void* commands);
    uint32_t (*wants)(void*, uint64_t frame);
    uint64_t (*readbacks)(void*);
    void (*target)(const rsf_sl_present_capture_packet*, rsf_dlss_capture_target*);
    int32_t (*images)(const rsf_sl_present_capture_packet*, uint64_t chain, uint32_t* count, uint64_t* images);
    uint32_t (*record)(void*, void* commands, const rsf_srvk_image*, const rsf_frame_record*, const char* role);
    /* Submits the copy behind the Present's original wait semaphores. */
    int32_t (*submit)(const rsf_sl_present_capture_packet*, void* commands, uint64_t fence);
    /* Zero waits presents without the semaphores the copy already consumed. */
    int32_t (*present)(const rsf_sl_present_capture_packet*, uint32_t waits);
    uint32_t (*flush)(void*);
    void (*failure)(void*, uint64_t source, const char* reason, int32_t result);
    void (*append)(void*, const char* file, const uint8_t* line, size_t length);
} rsf_dlss_capture_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* The platform outlives the owner. Registration is the last step of open.
   Detach before close; close follows presenter and GPU drain. */
rsf_backend_result rsf_core_dlss_capture_open(const rsf_dlss_capture_platform*, const rsf_fgvk_setup*, rsf_dlss_capture**);
void rsf_core_dlss_capture_remember(rsf_dlss_capture*, const rsf_frame_record*, uint32_t generated_count,
    const rsf_dlssgvk_status*, uint32_t configured);
/* The caller has already drained the presenter and device. */
void rsf_core_dlss_capture_flush(rsf_dlss_capture*);
rsf_backend_result rsf_core_dlss_capture_detach(rsf_dlss_capture*);
void rsf_core_dlss_capture_close(rsf_dlss_capture*);
#ifdef __cplusplus
}
#endif
