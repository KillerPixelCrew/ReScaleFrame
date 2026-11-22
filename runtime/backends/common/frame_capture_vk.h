/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <vulkan/vulkan.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Rust owns recording and readback lifetime. Callers still supply actual GPU
   completion evidence. A full flush requires retirement of all recorded work. */
#define RSF_CAPTURE_VK_SHARED_SUBMISSION 0u
#define RSF_CAPTURE_VK_COMMAND_RETIREMENT 1u

typedef struct rsf_core_capture_vk rsf_core_capture_vk;
rsf_core_capture_vk* rsf_core_capture_vk_create(uint32_t command_completion);
int32_t rsf_core_capture_vk_initialize(rsf_core_capture_vk*, void*, rsf_vk_get_proc,
    const VkPhysicalDeviceMemoryProperties*);
void rsf_core_capture_vk_destroy(rsf_core_capture_vk*);
int32_t rsf_core_capture_vk_enabled(const rsf_core_capture_vk*);
uint64_t rsf_core_capture_vk_pending(const rsf_core_capture_vk*);
void rsf_core_capture_vk_provider(const rsf_core_capture_vk*, uint64_t version, uint32_t backend, uint32_t generation);
int32_t rsf_core_capture_vk_wants(const rsf_core_capture_vk*, uint64_t frame);
int32_t rsf_core_capture_vk_record(const rsf_core_capture_vk*, void* commands, const rsf_srvk_image*,
    const rsf_frame_record*, const char* role, uint32_t reset, uint32_t core_source, uint64_t* ticket, uint64_t group);
void rsf_core_capture_vk_complete_group(const rsf_core_capture_vk*, uint64_t group);
void rsf_core_capture_vk_discard_group(const rsf_core_capture_vk*, uint64_t group);
void rsf_core_capture_vk_retire(const rsf_core_capture_vk*, void* commands);
int32_t rsf_core_capture_vk_flush(const rsf_core_capture_vk*, uint32_t ready_only, void* commands);

#ifdef __cplusplus
}
#endif
