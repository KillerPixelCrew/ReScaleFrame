// SPDX-License-Identifier: MIT
#pragma once

#include "bridge_api.h"

inline constexpr uint32_t RSF_XESS_BRIDGE_PLATFORM_ABI_VERSION = 1;

struct rsf_xess_bridge_session;

struct rsf_xess_bridge_modules {
    void* fg;
    void* xell;
};

// Private Windows x64 C ABI. The native capsule stores typed COM and SDK objects.
// Only the Rust session may retire the capsule and its Win32 leases. The host
// serializes start, completion-event use, and retirement against presentation.
struct rsf_xess_bridge_platform {
    uint32_t struct_size, abi_version;
    rsf_xess_bridge* (*allocate)(rsf_xess_bridge_session*);
    void (*release_native)(rsf_xess_bridge*);
    int32_t (*load_runtime)(rsf_xess_bridge*, const uint16_t*, rsf_xess_bridge_modules*);
    void (*unload_module)(void*);
    int32_t (*create_factory)(rsf_xess_bridge*);
    int32_t (*find_adapter)(rsf_xess_bridge*, uint64_t);
    int32_t (*create_device)(rsf_xess_bridge*);
    int32_t (*create_queue)(rsf_xess_bridge*);
    int32_t (*create_xell)(rsf_xess_bridge*);
    int32_t (*create_fg)(rsf_xess_bridge*, void*);
    int32_t (*set_latency)(rsf_xess_bridge*);
    int32_t (*set_default_sleep_mode)(rsf_xess_bridge*);
    void (*log_create_error)(const char*, int32_t, int32_t);
    int32_t (*init_swapchain)(rsf_xess_bridge*, void*, uint32_t, uint32_t, uint32_t, uint32_t);
    int32_t (*query_limit)(rsf_xess_bridge*, uint32_t*);
    int32_t (*get_swapchain)(rsf_xess_bridge*);
    int32_t (*enable_ui)(rsf_xess_bridge*);
    int32_t (*create_completion_fence)(rsf_xess_bridge*);
    void* (*open_event)();
    int32_t (*create_recorders)(rsf_xess_bridge*);
    int32_t (*start_capture)(rsf_xess_bridge*, void*);
    int32_t (*set_generated)(rsf_xess_bridge*, uint32_t);
    int32_t (*read_completion)(rsf_xess_bridge*, uint64_t*);
    int32_t (*arm_completion)(rsf_xess_bridge*, uint64_t, void*);
    int32_t (*wait_event)(void*, uint32_t);
    int32_t (*disable_fg)(rsf_xess_bridge*);
    int32_t (*flush_capture)(rsf_xess_bridge*);
    void (*release_swapchain)(rsf_xess_bridge*);
    int32_t (*destroy_fg)(rsf_xess_bridge*);
    int32_t (*destroy_xell)(rsf_xess_bridge*);
};

static_assert(sizeof(rsf_xess_bridge_modules) == 16);
static_assert(sizeof(rsf_xess_bridge_platform) == 248 && alignof(rsf_xess_bridge_platform) == 8);
static_assert(offsetof(rsf_xess_bridge_platform, allocate) == 8);
static_assert(offsetof(rsf_xess_bridge_platform, init_swapchain) == 112);
static_assert(offsetof(rsf_xess_bridge_platform, read_completion) == 184);
static_assert(offsetof(rsf_xess_bridge_platform, destroy_xell) == 240);

extern "C" {
int32_t rsf_core_xess_bridge_create(const rsf_xess_bridge_platform*, uint64_t, const uint16_t*, rsf_xess_bridge**);
const char* rsf_core_xess_bridge_create_stage();
void rsf_core_xess_bridge_note_vulkan_lookup();
int32_t rsf_core_xess_bridge_start(rsf_xess_bridge_session*, void*, uint32_t, uint32_t, uint32_t, uint32_t, int32_t);
uint32_t rsf_core_xess_bridge_started(const rsf_xess_bridge_session*);
uint32_t rsf_core_xess_bridge_max_generated(const rsf_xess_bridge_session*);
void* rsf_core_xess_bridge_event(const rsf_xess_bridge_session*);
int32_t rsf_core_xess_bridge_wait(const rsf_xess_bridge_session*, uint64_t);
void rsf_core_xess_bridge_retain_failure(const rsf_xess_bridge_session*, int32_t);
int32_t rsf_core_xess_bridge_prepare_destroy(rsf_xess_bridge_session*, uint64_t);
int32_t rsf_core_xess_bridge_destroy(rsf_xess_bridge_session*, uint64_t);
}
