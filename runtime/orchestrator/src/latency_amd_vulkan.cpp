// SPDX-License-Identifier: MIT
#include "latency_amd_vulkan_platform.h"
#include <cstddef>
#include <cstring>
#include <vector>

static_assert(RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_BACKEND_ERROR_ABI_MISMATCH == -2 &&
    RSF_BACKEND_ERROR_NOT_SUPPORTED == -7 && RSF_BACKEND_ERROR_NOT_READY == -8);

#if defined(RSF_HAVE_ANTILAG_VK)
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
namespace {
static_assert(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2 == 1000059000 &&
    VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD == 1000476000 &&
    sizeof(VkBaseOutStructure) == 16 && offsetof(VkBaseOutStructure, pNext) == 8 &&
    sizeof(VkPhysicalDeviceAntiLagFeaturesAMD) == 24 &&
    offsetof(VkPhysicalDeviceAntiLagFeaturesAMD, antiLag) == 16 && VK_TRUE == 1);
template<class T> T function(const rsf_latencyvk_device_query& q,const char* name) {
    return reinterpret_cast<T>(q.instance_proc(q.instance,name));
}
int32_t vendor(const rsf_latencyvk_device_query* query, uint32_t* out) noexcept try {
    const auto properties = function<PFN_vkGetPhysicalDeviceProperties>(*query, "vkGetPhysicalDeviceProperties");
    if (!properties) return RSF_BACKEND_OK;
    VkPhysicalDeviceProperties value{};
    properties(static_cast<VkPhysicalDevice>(query->physical_device), &value);
    *out = value.vendorID;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
// The Rust owner passes presentation facts and the already converted limiter.
static_assert(VK_ANTI_LAG_STAGE_INPUT_AMD == 0 && VK_ANTI_LAG_STAGE_PRESENT_AMD == 1);
void update_device(void* device, rsf_vk_function proc, uint32_t stage, uint64_t frame,
    uint32_t enabled, uint32_t max_fps) {
    VkAntiLagPresentationInfoAMD present{VK_STRUCTURE_TYPE_ANTI_LAG_PRESENTATION_INFO_AMD};
    present.stage=stage==VK_ANTI_LAG_STAGE_PRESENT_AMD ? VK_ANTI_LAG_STAGE_PRESENT_AMD : VK_ANTI_LAG_STAGE_INPUT_AMD;
    present.frameIndex=frame;
    VkAntiLagDataAMD data{VK_STRUCTURE_TYPE_ANTI_LAG_DATA_AMD};
    data.mode=enabled ? VK_ANTI_LAG_MODE_ON_AMD : VK_ANTI_LAG_MODE_OFF_AMD;
    data.maxFPS=max_fps;
    data.pPresentationInfo=stage==UINT32_MAX ? nullptr : &present;
    reinterpret_cast<PFN_vkAntiLagUpdateAMD>(proc)(static_cast<VkDevice>(device),&data);
}
int32_t supported(const rsf_latencyvk_device_query* query, uint32_t* out) noexcept try {
    *out = 0;
    const auto enumerate=function<PFN_vkEnumerateDeviceExtensionProperties>(*query,"vkEnumerateDeviceExtensionProperties");
    auto get_features=function<PFN_vkGetPhysicalDeviceFeatures2>(*query,"vkGetPhysicalDeviceFeatures2");
    if (!get_features) get_features=function<PFN_vkGetPhysicalDeviceFeatures2>(*query,"vkGetPhysicalDeviceFeatures2KHR");
    if (!enumerate || !get_features) return RSF_BACKEND_OK;
    uint32_t count{};const auto physical=static_cast<VkPhysicalDevice>(query->physical_device);
    if (enumerate(physical,nullptr,&count,nullptr)!=VK_SUCCESS || count>4096) return RSF_BACKEND_OK;
    std::vector<VkExtensionProperties> supported(count);
    if (enumerate(physical,nullptr,&count,supported.data())!=VK_SUCCESS || count>supported.size()) return RSF_BACKEND_OK;
    bool available=false;
    for (uint32_t i=0;i<count;++i) if (std::strcmp(supported[i].extensionName,VK_AMD_ANTI_LAG_EXTENSION_NAME)==0) available=true;
    if (!available) return RSF_BACKEND_OK;
    VkPhysicalDeviceAntiLagFeaturesAMD supported_feature{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD};
    VkPhysicalDeviceFeatures2 root{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};root.pNext=&supported_feature;
    get_features(physical,&root);
    *out = supported_feature.antiLag;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void device_info(const rsf_latencyvk_created_device* facts, rsf_amd_vk_device_info* out) noexcept {
    const auto& info = *static_cast<const VkDeviceCreateInfo*>(facts->create_info);
    *out = {info.pNext, info.enabledExtensionCount, info.ppEnabledExtensionNames};
}

int32_t device_update(const rsf_latencyvk_created_device* facts, rsf_vk_function* out) noexcept try {
    const auto get_device = reinterpret_cast<PFN_vkGetDeviceProcAddr>(
        facts->instance_proc(facts->instance, "vkGetDeviceProcAddr"));
    *out = get_device ? reinterpret_cast<rsf_vk_function>(
        get_device(static_cast<VkDevice>(facts->device), "vkAntiLagUpdateAMD")) : nullptr;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

constexpr rsf_amd_vk_platform leaves{vendor, supported, device_info, device_update, update_device};
constexpr const rsf_amd_vk_platform* platform = &leaves;
}
#else
namespace {
constexpr const rsf_amd_vk_platform* platform = nullptr;
}
#endif

extern "C" rsf_backend_result rsf_latencyvk_require_device(const rsf_latencyvk_device_query* query,
    rsf_latencyvk_device_requirements* out) {
    return rsf_core_amd_vk_require(platform, query, out);
}

extern "C" rsf_backend_result rsf_latencyvk_device_created(const rsf_latencyvk_created_device* facts) {
    return rsf_core_amd_vk_device_created(platform, facts);
}

extern "C" void rsf_latencyvk_device_destroyed(void* device) {
    rsf_core_amd_vk_device_destroyed(device);
}
