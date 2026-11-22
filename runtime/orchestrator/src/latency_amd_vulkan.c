// SPDX-License-Identifier: MIT
#include <rescaleframe/native_latency_vulkan_requirements.h>

int32_t rsf_core_amd_vk_require(uint32_t enabled, const rsf_latencyvk_device_query*,
    rsf_latencyvk_device_requirements*);
int32_t rsf_core_amd_vk_device_created(uint32_t enabled, const rsf_latencyvk_created_device*);
void rsf_core_amd_vk_device_destroyed(void*);

#if defined(RSF_HAVE_ANTILAG_VK)
static const uint32_t enabled = 1;
#else
static const uint32_t enabled = 0;
#endif

rsf_backend_result rsf_latencyvk_require_device(const rsf_latencyvk_device_query* query,
    rsf_latencyvk_device_requirements* out) {
    return rsf_core_amd_vk_require(enabled, query, out);
}

rsf_backend_result rsf_latencyvk_device_created(const rsf_latencyvk_created_device* facts) {
    return rsf_core_amd_vk_device_created(enabled, facts);
}

void rsf_latencyvk_device_destroyed(void* device) {
    rsf_core_amd_vk_device_destroyed(device);
}
