// SPDX-License-Identifier: MIT
#include "latency_amd_vulkan_platform.h"
#include <cstring>
#include <vector>

static_assert(RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_BACKEND_ERROR_ABI_MISMATCH == -2 &&
    RSF_BACKEND_ERROR_NOT_SUPPORTED == -7 && RSF_BACKEND_ERROR_NOT_READY == -8);

#if defined(RSF_HAVE_ANTILAG_VK)
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
namespace {
thread_local VkPhysicalDeviceAntiLagFeaturesAMD feature{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD};
bool extension(uint32_t count,const char* const* names,const char* wanted) {
    if (count>4096 || (count && !names)) return false;
    for (uint32_t i=0;i<count;++i) if (names[i] && std::strcmp(names[i],wanted)==0) return true;
    return false;
}
template<class T> T function(const rsf_latencyvk_device_query& q,const char* name) {
    return reinterpret_cast<T>(q.instance_proc(q.instance,name));
}
VkBaseOutStructure* find(void* chain,VkStructureType type) {
    uint32_t count=0;
    for (auto* node=static_cast<VkBaseOutStructure*>(chain);node && ++count<=64;node=node->pNext)
        if (node->sType==type) return node;
    return nullptr;
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
int32_t enable(const rsf_latencyvk_device_query* query, rsf_latencyvk_device_requirements* out) noexcept try {
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
    if (!supported_feature.antiLag) return RSF_BACKEND_OK;
    if (auto* existing=find(query->features,supported_feature.sType)) {
        reinterpret_cast<VkPhysicalDeviceAntiLagFeaturesAMD*>(existing)->antiLag=VK_TRUE;
    } else {
        feature=supported_feature;
        if (auto* root_node=find(query->features,VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2)) {
            feature.pNext=root_node->pNext;root_node->pNext=reinterpret_cast<VkBaseOutStructure*>(&feature);
        } else {feature.pNext=query->features;out->features=&feature;}
    }
    static const char* names[]{VK_AMD_ANTI_LAG_EXTENSION_NAME};out->extensions=names;out->extension_count=1;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t device_update(const rsf_latencyvk_created_device* facts, rsf_vk_function* out) noexcept try {
    const auto& info = *static_cast<const VkDeviceCreateInfo*>(facts->create_info);
    const auto* enabled = reinterpret_cast<const VkPhysicalDeviceAntiLagFeaturesAMD*>(
        find(const_cast<void*>(info.pNext), VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD));
    if (!extension(info.enabledExtensionCount, info.ppEnabledExtensionNames, VK_AMD_ANTI_LAG_EXTENSION_NAME) ||
        !enabled || !enabled->antiLag) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const auto get_device = reinterpret_cast<PFN_vkGetDeviceProcAddr>(
        facts->instance_proc(facts->instance, "vkGetDeviceProcAddr"));
    *out = get_device ? reinterpret_cast<rsf_vk_function>(
        get_device(static_cast<VkDevice>(facts->device), "vkAntiLagUpdateAMD")) : nullptr;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

constexpr rsf_amd_vk_platform leaves{vendor, enable, device_update, update_device};
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
