// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr_split_vulkan.h>
#if defined(RSF_VULKAN_FSR4_ENABLED)
#include "native_sr_fsr4_vulkan.h"
#endif

namespace {
struct Provider {
    int32_t (*create)(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
    int32_t (*plan)(const rsf_fsr4vk*, uint32_t*, uint32_t*);
    int32_t (*evaluate)(rsf_fsr4vk*, void*, const rsf_srvk_frame*, void**);
    int32_t (*destroy)(rsf_fsr4vk*);
    void (*retire)(rsf_fsr4vk*, void*);
    int32_t (*submitted)(rsf_fsr4vk*, void*);
    int32_t (*info)(const rsf_fsr4vk*, rsf_srvk_fsr4_provider_info*);
};

#if defined(RSF_VULKAN_FSR4_ENABLED)
template<auto Call, class... Args>
int32_t invoke(Args... args) noexcept try
{
    return Call(args...);
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t read_info(const rsf_fsr4vk* owner, rsf_srvk_fsr4_provider_info* out) noexcept
{
    rsf_fsr4_bridge_info native{sizeof(native), RSF_FSR4_BRIDGE_ABI_VERSION};
    const auto result = invoke<rsf_fsr4vk_info>(owner, &native);
    if (result == RSF_BACKEND_OK) {
        out->adapter_luid = native.adapter_luid;
        out->provider_version = native.provider_version;
        out->vendor_id = native.vendor_id;
        out->int8_compatibility = native.int8_compatibility;
    }
    return result;
}

void retire_commands(rsf_fsr4vk* owner, void* commands) noexcept try
{
    rsf_fsr4vk_retire_commands(owner, commands);
}
catch (...) {}

constexpr Provider native{
    invoke<rsf_fsr4vk_create>, invoke<rsf_fsr4vk_plan>, invoke<rsf_fsr4vk_evaluate>,
    invoke<rsf_fsr4vk_destroy>, retire_commands, invoke<rsf_fsr4vk_submitted>, read_info,
};
constexpr const Provider* provider = &native;
#else
constexpr const Provider* provider = nullptr;
#endif
}

extern "C" {
int32_t rsf_core_srvk_fsr4_create(const Provider*, const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
int32_t rsf_core_srvk_fsr4_plan(const Provider*, const rsf_fsr4vk*, uint32_t*, uint32_t*);
int32_t rsf_core_srvk_fsr4_evaluate(const Provider*, rsf_fsr4vk*, void*, const rsf_srvk_frame*, void**);
int32_t rsf_core_srvk_fsr4_destroy(const Provider*, rsf_fsr4vk*);
void rsf_core_srvk_fsr4_retire_commands(const Provider*, rsf_fsr4vk*, void*);
int32_t rsf_core_srvk_fsr4_submitted(const Provider*, rsf_fsr4vk*, void*);
int32_t rsf_core_srvk_fsr4_info(const Provider*, const rsf_fsr4vk*, rsf_srvk_fsr4_provider_info*);

int32_t rsf_srvk_fsr4_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** owner)
{
    return rsf_core_srvk_fsr4_create(provider, setup, handoff, owner);
}

int32_t rsf_srvk_fsr4_plan(const rsf_fsr4vk* owner, uint32_t* width, uint32_t* height)
{
    return rsf_core_srvk_fsr4_plan(provider, owner, width, height);
}

int32_t rsf_srvk_fsr4_evaluate(rsf_fsr4vk* owner, void* commands, const rsf_srvk_frame* frame, void** continuation)
{
    return rsf_core_srvk_fsr4_evaluate(provider, owner, commands, frame, continuation);
}

int32_t rsf_srvk_fsr4_destroy(rsf_fsr4vk* owner)
{
    return rsf_core_srvk_fsr4_destroy(provider, owner);
}

void rsf_srvk_fsr4_retire_commands(rsf_fsr4vk* owner, void* commands)
{
    rsf_core_srvk_fsr4_retire_commands(provider, owner, commands);
}

int32_t rsf_srvk_fsr4_submitted(rsf_fsr4vk* owner, void* commands)
{
    return rsf_core_srvk_fsr4_submitted(provider, owner, commands);
}

int32_t rsf_srvk_fsr4_info(const rsf_fsr4vk* owner, rsf_srvk_fsr4_provider_info* info)
{
    return rsf_core_srvk_fsr4_info(provider, owner, info);
}
}
