// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr_split_vulkan.h>

typedef struct rsf_split_provider {
    int32_t (*create)(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
    int32_t (*plan)(const rsf_fsr4vk*, uint32_t*, uint32_t*);
    int32_t (*evaluate)(rsf_fsr4vk*, void*, const rsf_srvk_frame*, void**);
    int32_t (*destroy)(rsf_fsr4vk*);
    void (*retire)(rsf_fsr4vk*, void*);
    int32_t (*submitted)(rsf_fsr4vk*, void*);
    int32_t (*info)(const rsf_fsr4vk*, rsf_srvk_fsr4_provider_info*);
} rsf_split_provider;

#if defined(RSF_VULKAN_FSR4_ENABLED)
#include "native_sr_fsr4_vulkan.h"

int32_t rsf_core_fsr4vk_public_info(const rsf_fsr4vk*, rsf_srvk_fsr4_provider_info*);

// The typed provider is optional in CMake. The Rust archive also links into
// targets that omit the Vulkan graphics leaves.
static const rsf_split_provider native = {
    rsf_fsr4vk_create_admitted, rsf_fsr4vk_plan, rsf_fsr4vk_evaluate,
    rsf_fsr4vk_destroy, rsf_fsr4vk_retire_commands, rsf_fsr4vk_submitted, rsf_core_fsr4vk_public_info,
};
#define RSF_SPLIT_PROVIDER (&native)
#else
#define RSF_SPLIT_PROVIDER ((const rsf_split_provider*)0)
#endif

int32_t rsf_core_srvk_fsr4_create(const rsf_split_provider*, const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
int32_t rsf_core_srvk_fsr4_plan(const rsf_split_provider*, const rsf_fsr4vk*, uint32_t*, uint32_t*);
int32_t rsf_core_srvk_fsr4_evaluate(const rsf_split_provider*, rsf_fsr4vk*, void*, const rsf_srvk_frame*, void**);
int32_t rsf_core_srvk_fsr4_destroy(const rsf_split_provider*, rsf_fsr4vk*);
void rsf_core_srvk_fsr4_retire_commands(const rsf_split_provider*, rsf_fsr4vk*, void*);
int32_t rsf_core_srvk_fsr4_submitted(const rsf_split_provider*, rsf_fsr4vk*, void*);
int32_t rsf_core_srvk_fsr4_info(const rsf_split_provider*, const rsf_fsr4vk*, rsf_srvk_fsr4_provider_info*);

int32_t rsf_srvk_fsr4_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** owner)
{
    return rsf_core_srvk_fsr4_create(RSF_SPLIT_PROVIDER, setup, handoff, owner);
}

int32_t rsf_srvk_fsr4_plan(const rsf_fsr4vk* owner, uint32_t* width, uint32_t* height)
{
    return rsf_core_srvk_fsr4_plan(RSF_SPLIT_PROVIDER, owner, width, height);
}

int32_t rsf_srvk_fsr4_evaluate(rsf_fsr4vk* owner, void* commands, const rsf_srvk_frame* frame, void** continuation)
{
    return rsf_core_srvk_fsr4_evaluate(RSF_SPLIT_PROVIDER, owner, commands, frame, continuation);
}

int32_t rsf_srvk_fsr4_destroy(rsf_fsr4vk* owner)
{
    return rsf_core_srvk_fsr4_destroy(RSF_SPLIT_PROVIDER, owner);
}

void rsf_srvk_fsr4_retire_commands(rsf_fsr4vk* owner, void* commands)
{
    rsf_core_srvk_fsr4_retire_commands(RSF_SPLIT_PROVIDER, owner, commands);
}

int32_t rsf_srvk_fsr4_submitted(rsf_fsr4vk* owner, void* commands)
{
    return rsf_core_srvk_fsr4_submitted(RSF_SPLIT_PROVIDER, owner, commands);
}

int32_t rsf_srvk_fsr4_info(const rsf_fsr4vk* owner, rsf_srvk_fsr4_provider_info* info)
{
    return rsf_core_srvk_fsr4_info(RSF_SPLIT_PROVIDER, owner, info);
}