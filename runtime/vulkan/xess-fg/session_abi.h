// SPDX-License-Identifier: MIT
#pragma once

#include "bridge_api.h"

inline constexpr uint32_t RSF_XESS_BRIDGE_PLATFORM_ABI_VERSION = 6;
inline constexpr uint32_t RSF_XESS_BRIDGE_ALLOCATOR_COUNT = 3;

struct rsf_xess_bridge_session;
struct VulkanStoryXessFrame;
// Rust capture owner and the records shared with it, defined in diagnostic-capture.h.
struct rsf_xess_capture_owner;
struct rsf_xess_capture_desc;

struct rsf_xess_bridge_present_result {
    uint32_t frames_presented, fg_enabled;
    int32_t fg_result;
};

struct rsf_xess_bridge_resource_desc {
    uint64_t width;
    uint32_t height, format;
};

struct rsf_xess_bridge_modules {
    void* fg;
    void* xell;
};

struct rsf_xess_timing_gpu_sample {
    uint64_t start, copies_end, end, frequency, calibration_gpu, calibration_cpu;
    int64_t present_enter, present_exit;
};

struct rsf_xess_timing_report {
    uint32_t presents, gpu_samples, sequence_count, reserved;
    double phase_mean[6], phase_max[6];
    double gpu_mean, gpu_max, gpu_start_after_present_enter, present_exit_after_gpu_end, copies_mean;
    double sequence[24];
};

static_assert(sizeof(rsf_xess_timing_gpu_sample) == 64);
static_assert(sizeof(rsf_xess_timing_report) == 344 && offsetof(rsf_xess_timing_report, sequence) == 152);
struct rsf_xess_timing;
extern "C" void rsf_core_xess_timing_cpu(const rsf_xess_timing*, uint32_t phase, int64_t start, int64_t end, int64_t frequency);
extern "C" void rsf_core_xess_timing_gpu(const rsf_xess_timing*, const rsf_xess_timing_gpu_sample*, int64_t cpu_frequency);

// Private Windows x64 C ABI. The native capsule stores typed COM and SDK objects.
// Only the Rust session may retire the capsule and its Win32 leases. The host
// serializes start, completion-event use, and retirement against presentation.
struct rsf_xess_bridge_platform {
    uint32_t struct_size, abi_version;
    rsf_xess_bridge* (*allocate)(rsf_xess_bridge_session*, const rsf_xess_timing*);
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
    int32_t (*find_diagnostics)(rsf_xess_bridge*, void*);
    int32_t (*set_generated)(rsf_xess_bridge*, uint32_t);
    int32_t (*read_completion)(rsf_xess_bridge*, uint64_t*);
    int32_t (*arm_completion)(rsf_xess_bridge*, uint64_t, void*);
    int32_t (*wait_event)(void*, uint32_t);
    int32_t (*disable_fg)(rsf_xess_bridge*);
    int32_t (*unregister_diagnostics)(rsf_xess_bridge*);
    void (*release_swapchain)(rsf_xess_bridge*);
    int32_t (*destroy_fg)(rsf_xess_bridge*);
    int32_t (*destroy_xell)(rsf_xess_bridge*);
    uint32_t (*has_shared_queue)(const rsf_xess_bridge*);
    void (*describe_present_resource)(void* resource, rsf_xess_bridge_resource_desc*);
    int32_t (*reset_commands)(rsf_xess_bridge*, uint32_t);
    void (*transition_present_resource)(rsf_xess_bridge*, void* resource, uint32_t before, uint32_t after);
    // Transfers one DXGI backbuffer reference to the session on success.
    int32_t (*acquire_backbuffer)(rsf_xess_bridge*, void** backbuffer);
    void (*copy_present_color)(rsf_xess_bridge*, void* backbuffer, void* color);
    int32_t (*tag_present_resource)(rsf_xess_bridge*, void* resource, uint32_t role, uint32_t frame_id);
    void (*release_backbuffer)(void* backbuffer);
    int32_t (*close_commands)(rsf_xess_bridge*);
    int32_t (*tag_constants)(rsf_xess_bridge*, const VulkanStoryXessFrame*);
    int32_t (*wait_shared)(rsf_xess_bridge*, uint64_t);
    void (*execute)(rsf_xess_bridge*);
    int32_t (*set_present_id)(rsf_xess_bridge*, uint32_t);
    int32_t (*present)(rsf_xess_bridge*, uint32_t);
    int32_t (*signal_done)(rsf_xess_bridge*, uint64_t);
    int32_t (*signal_completion)(rsf_xess_bridge*, uint64_t);
    int32_t (*read_present_status)(rsf_xess_bridge*, rsf_xess_bridge_present_result*);
    uint32_t (*timing_enabled)();
    int64_t (*timing_now)();
    int64_t (*timing_frequency)();
    void (*timing_report)(const rsf_xess_timing_report*);
    uint32_t (*begin_gpu_timing)(rsf_xess_bridge*, uint32_t);
    void (*timestamp_copies)(rsf_xess_bridge*, uint32_t, uint32_t);
    void (*mark_present_enter)(rsf_xess_bridge*, uint32_t);
    void (*finish_gpu_timing)(rsf_xess_bridge*, uint32_t);
    uint32_t (*serialize_enabled)();
    // Opt-in diagnostic capture leaves. Rust owns admission, readbacks and
    // publication. These callbacks keep the typed SDK and DX12 calls.
    void (*attach_capture)(rsf_xess_bridge*, rsf_xess_capture_owner*);
    int32_t (*register_diagnostics)(rsf_xess_bridge*);
    uint32_t (*capture_fingerprint)(void*);
    void (*capture_describe)(void*, rsf_xess_capture_desc*);
    int32_t (*capture_footprint)(void*, void*, uint64_t*, void**);
    void* (*capture_readback)(void*, uint64_t);
    void (*capture_release)(void*);
    void (*capture_copy)(void*, void*, void*, const void*, uint32_t);
    int32_t (*capture_map)(void*, uint64_t, const uint8_t**);
    void (*capture_unmap)(void*);
};

static_assert(sizeof(rsf_xess_bridge_modules) == 16);
static_assert(sizeof(rsf_xess_bridge_present_result) == 12);
static_assert(sizeof(rsf_xess_bridge_resource_desc) == 16 && alignof(rsf_xess_bridge_resource_desc) == 8);
static_assert(offsetof(rsf_xess_bridge_resource_desc, height) == 8);
static_assert(offsetof(rsf_xess_bridge_resource_desc, format) == 12);
static_assert(sizeof(rsf_xess_bridge_platform) == 536 && alignof(rsf_xess_bridge_platform) == 8);
static_assert(offsetof(rsf_xess_bridge_platform, allocate) == 8);
static_assert(offsetof(rsf_xess_bridge_platform, init_swapchain) == 112);
static_assert(offsetof(rsf_xess_bridge_platform, find_diagnostics) == 168);
static_assert(offsetof(rsf_xess_bridge_platform, read_completion) == 184);
static_assert(offsetof(rsf_xess_bridge_platform, unregister_diagnostics) == 216);
static_assert(offsetof(rsf_xess_bridge_platform, destroy_xell) == 240);
static_assert(offsetof(rsf_xess_bridge_platform, has_shared_queue) == 248);
static_assert(offsetof(rsf_xess_bridge_platform, describe_present_resource) == 256);
static_assert(offsetof(rsf_xess_bridge_platform, transition_present_resource) == 272);
static_assert(offsetof(rsf_xess_bridge_platform, acquire_backbuffer) == 280);
static_assert(offsetof(rsf_xess_bridge_platform, tag_present_resource) == 296);
static_assert(offsetof(rsf_xess_bridge_platform, release_backbuffer) == 304);
static_assert(offsetof(rsf_xess_bridge_platform, wait_shared) == 328);
static_assert(offsetof(rsf_xess_bridge_platform, read_present_status) == 376);
static_assert(offsetof(rsf_xess_bridge_platform, serialize_enabled) == 448);
static_assert(offsetof(rsf_xess_bridge_platform, attach_capture) == 456);
static_assert(offsetof(rsf_xess_bridge_platform, capture_footprint) == 488);
static_assert(offsetof(rsf_xess_bridge_platform, capture_unmap) == 528);

extern "C" {
int32_t rsf_core_xess_bridge_create(const rsf_xess_bridge_platform*, uint64_t, const uint16_t*, rsf_xess_bridge**);
int32_t rsf_core_xess_bridge_create_integrated(const rsf_xess_bridge_platform*, const rsf_xess_bridge_create*, rsf_xess_bridge**);
const char* rsf_core_xess_bridge_create_stage();
void rsf_core_xess_bridge_note_vulkan_lookup();
int32_t rsf_core_xess_bridge_start(rsf_xess_bridge_session*, void*, uint32_t, uint32_t, uint32_t, uint32_t, int32_t);
int32_t rsf_core_xess_bridge_start_integrated(rsf_xess_bridge_session*, const rsf_xess_bridge_start*, int32_t);
uint32_t rsf_core_xess_bridge_started(const rsf_xess_bridge_session*);
uint32_t rsf_core_xess_bridge_max_generated(const rsf_xess_bridge_session*);
uint32_t rsf_core_xess_bridge_active_generated(const rsf_xess_bridge_session*);
int32_t rsf_core_xess_bridge_set_generated(rsf_xess_bridge_session*, uint32_t, int32_t);
int32_t rsf_core_xess_bridge_set_generated_legacy(rsf_xess_bridge_session*, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_xess_bridge_wait_idle(const rsf_xess_bridge_session*);
int32_t rsf_core_xess_bridge_present(rsf_xess_bridge_session*, const VulkanStoryXessFrame*, uint32_t*, int32_t*, uint32_t*);
int32_t rsf_core_xess_bridge_present_integrated(rsf_xess_bridge_session*, const rsf_xess_bridge_frame*, rsf_xess_bridge_present_status*);
int32_t rsf_core_xess_bridge_prepare_destroy(rsf_xess_bridge_session*);
int32_t rsf_core_xess_bridge_destroy(rsf_xess_bridge_session*);
}
