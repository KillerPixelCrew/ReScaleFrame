/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include "results.h"
#include "../../runtime/orchestrator/src/native_fg_ffx_vulkan.h"
#include "../../runtime/orchestrator/src/vulkan_fg_resources.h"

typedef struct rsf_ffx_fixture_state {
    uint64_t next;
    uint32_t resets, submits, waits, copies, sdk_records, generated;
    uint32_t configurations, activations, present_calls, drains, idles, locks, unlocks;
    uint32_t effects_destroyed, swapchains_destroyed, chains_destroyed;
    uint32_t destroyed_images, freed_memories, destroyed_fences;
    bool reject_prepare, reject_wait, reject_bind, reject_drain, reject_effect_destroy;
    bool reject_swapchain_destroy, reject_generate, check_reset, expected_reset, enabled, reject_submit;
    uint64_t configured_frame, generated_frame;
    int32_t present_result;
} rsf_ffx_fixture_state;

typedef struct rsf_wsi_route_fixture_state {
    rsf_ffxgvk* owner;
    uint32_t presents, creates, destroys, acquires, enumerations, lookups, unregistered, registered;
    bool destroy_inside, refuse_unregister;
    int32_t destroyed_inside, created_inside;
    void* routed_inside;
} rsf_wsi_route_fixture_state;

#ifdef __cplusplus
extern "C" {
#endif
rsf_ffx_fixture_state* rsf_fixture_ffx_state(void);
rsf_vk_function rsf_fixture_ffx_lookup(void*, const char*);
int32_t rsf_fixture_ffx_submit(void*, uint32_t, const void*, uint64_t);
void rsf_fixture_ffx_lock(void*);
void rsf_fixture_ffx_unlock(void*);
rsf_wsi_route_fixture_state* rsf_fixture_wsi_route_state(void);
int32_t rsf_fixture_ffx_refused_second_owner(void);
rsf_vk_function rsf_fixture_wsi_lookup(void*, const char*);
rsf_vk_function rsf_fixture_wsi_entry(const char*);
int32_t rsf_fixture_ffx_start(bool route, rsf_ffxgvk**, uint64_t*);
void* rsf_fixture_ffx_native(void);
void* rsf_fixture_ffx_routed(void);
uint64_t rsf_fixture_ffx_chain(void);
void* rsf_fixture_ffx_gpu(void);
const rsf_vkfg_platform* rsf_fixture_ffx_platform(void);
int32_t rsf_fixture_wsi_create(uint64_t, uint64_t*);
void rsf_fixture_wsi_destroy(uint64_t, uint64_t);
int32_t rsf_fixture_wsi_acquire(uint64_t, uint64_t, uint32_t*);
int32_t rsf_fixture_wsi_images(uint64_t, uint64_t, uint32_t*);
int32_t rsf_fixture_wsi_present(uint64_t);
int32_t rsf_fixture_wsi_device_proc(uint64_t);
int32_t rsf_fixture_wsi_graphics_submit(uint64_t);
#ifdef __cplusplus
}
#endif
