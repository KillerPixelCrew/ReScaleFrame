/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <rescaleframe/native_fg_dlss_vulkan.h>
#include "results.h"
#include "../../runtime/orchestrator/src/native_fg_dlss_vulkan_capture_ledger.h"

enum rsf_dlss_fixture_variant {
    RSF_DLSS_FIXTURE_NORMAL = 0,
    RSF_DLSS_FIXTURE_CAPTURE_REQUIRED = 1,
    RSF_DLSS_FIXTURE_CAPTURE_REFUSED = 2,
};

typedef struct rsf_dlss_fixture_state {
    uint64_t lease_session, lease_frame;
    uint32_t on_calls, off_calls, present_calls, fence_submits, destroyed_chains;
    uint32_t sdk_presented, copies, release_calls, max_generated, backbuffer_count;
    uint32_t surface_usage, expected_chain_usage, chain_creates, inputs_waits;
    bool refuse_create, refuse_enumeration, refuse_options, refuse_submit, refuse_tags, refuse_release;
    bool worker_timeout, refuse_inputs_wait;
    uint64_t expected_inputs_value, completion_value;
} rsf_dlss_fixture_state;

typedef struct rsf_dlss_fixture_capture_calls {
    uintptr_t present, images, submit, wsi;
} rsf_dlss_fixture_capture_calls;

typedef struct rsf_dlss_fixture_capture_state {
    void* user;
    uint32_t registered, registrations;
    uint32_t pool_family, pool_flags, begins, ends, destroyed_pools;
    uint32_t real_submits, real_enumerations, real_presents, wsi_calls;
    uintptr_t waits, commands, submitted_fence, waited_fence, destroyed_fence;
    uint32_t wait_count, command_count, stages[2];
    uint64_t timeout;
    uint32_t flushes, metadata;
    uintptr_t present_info, present_waits, present_extension, present_chains, present_indices;
    uint32_t present_wait_count, present_chain_count;
    const char* failure;
    int32_t failure_result;
} rsf_dlss_fixture_capture_state;

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_fixture_dlss_locked(void);
rsf_dlss_fixture_state* rsf_fixture_dlss_state(void);
rsf_vk_function rsf_fixture_dlss_proc(void*, const char*);
int32_t rsf_fixture_dlss_submit(void*, uint32_t, const void*, uint64_t);
void rsf_fixture_dlss_lock(void*);
void rsf_fixture_dlss_unlock(void*);
rsf_backend_result rsf_fixture_dlss_create(enum rsf_dlss_fixture_variant, const rsf_fgvk_setup*, rsf_dlssgvk**, uint64_t*);
int32_t rsf_fixture_dlss_capture_open(const rsf_fgvk_setup*, rsf_dlss_capture**, rsf_dlss_fixture_capture_calls*);
void rsf_fixture_dlss_capture_state(rsf_dlss_fixture_capture_state*);
int32_t rsf_fixture_dlss_capture_call(const rsf_sl_present_capture_packet*);
void rsf_fixture_dlss_capture_calls_init(rsf_dlss_fixture_capture_calls*);
int32_t rsf_fixture_dlss_register_capture(const rsf_sl_present_capture_registration*);
uint32_t rsf_fixture_dlss_record(void*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*);
uint64_t rsf_fixture_dlss_readbacks(void*);
uint32_t rsf_fixture_dlss_flush(void*);
void rsf_fixture_dlss_failure(void*, uint64_t, const char*, int32_t);
void rsf_fixture_dlss_append(void*, const char*, const uint8_t*, size_t);
void rsf_fixture_dlss_reset_waited_fence(void);
int32_t rsf_fixture_dlss_run(void);
#ifdef __cplusplus
}
#endif
