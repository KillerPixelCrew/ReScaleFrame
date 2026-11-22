/* SPDX-License-Identifier: MIT */
#pragma once
#include "../runtime/orchestrator/src/native_sr_fsr4_vulkan.h"

typedef struct rsf_fsr4sr_fixture_calls {
    int32_t (*create)(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
    int32_t (*plan)(const rsf_fsr4vk*, uint32_t*, uint32_t*);
    int32_t (*info)(const rsf_fsr4vk*, rsf_fsr4_bridge_info*);
    int32_t (*evaluate)(rsf_fsr4vk*, void*, const rsf_srvk_frame*, void**);
    void (*retire)(rsf_fsr4vk*, void*);
    int32_t (*submitted)(rsf_fsr4vk*, void*);
    int32_t (*destroy)(rsf_fsr4vk*);
} rsf_fsr4sr_fixture_calls;
