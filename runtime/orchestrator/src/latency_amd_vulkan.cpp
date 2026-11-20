// SPDX-License-Identifier: MIT
#include <rescaleframe/native_latency_vulkan_requirements.h>
#include <cstring>
#include <vector>
extern "C" rsf_backend_result rsf_core_amd_vk_bind(void*, rsf_vk_function,
    void (*invoke)(void*, rsf_vk_function, uint32_t, uint64_t, uint32_t, uint32_t));
extern "C" void rsf_core_amd_vk_device_destroyed(void*);
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
bool amd(const rsf_latencyvk_device_query& q) {
    const auto properties=function<PFN_vkGetPhysicalDeviceProperties>(q,"vkGetPhysicalDeviceProperties");
    if (!properties) return false;
    VkPhysicalDeviceProperties value{};properties(static_cast<VkPhysicalDevice>(q.physical_device),&value);
    return value.vendorID==0x1002;
}
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
}
#endif
extern "C" rsf_backend_result rsf_latencyvk_require_device(const rsf_latencyvk_device_query* query,rsf_latencyvk_device_requirements* out) try {
    if (!query || !out || query->struct_size<sizeof(*query) || out->struct_size<sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (query->abi_version!=RSF_LATENCYVK_DEVICE_ABI_VERSION || out->abi_version!=RSF_LATENCYVK_DEVICE_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!query->instance || !query->physical_device || !query->instance_proc) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out={sizeof(*out),RSF_LATENCYVK_DEVICE_ABI_VERSION,0,nullptr,query->features};
#if defined(RSF_HAVE_ANTILAG_VK)
    if (!amd(*query)) return RSF_BACKEND_OK;
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
#endif
    return RSF_BACKEND_OK;
} catch (...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" rsf_backend_result rsf_latencyvk_device_created(const rsf_latencyvk_created_device* facts) try {
    if (!facts || facts->struct_size<sizeof(*facts) || !facts->device || !facts->create_info) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (facts->abi_version!=RSF_LATENCYVK_DEVICE_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_ANTILAG_VK)
    const rsf_latencyvk_device_query query{sizeof(query),RSF_LATENCYVK_DEVICE_ABI_VERSION,facts->instance,facts->physical_device,facts->instance_proc,nullptr};
    if (!query.instance || !query.physical_device || !query.instance_proc || !amd(query)) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const auto& info=*static_cast<const VkDeviceCreateInfo*>(facts->create_info);
    const auto* enabled=reinterpret_cast<const VkPhysicalDeviceAntiLagFeaturesAMD*>(find(const_cast<void*>(info.pNext),VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD));
    if (!extension(info.enabledExtensionCount,info.ppEnabledExtensionNames,VK_AMD_ANTI_LAG_EXTENSION_NAME) || !enabled || !enabled->antiLag) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const auto get_device=function<PFN_vkGetDeviceProcAddr>(query,"vkGetDeviceProcAddr");
    const auto callback=get_device ? reinterpret_cast<PFN_vkAntiLagUpdateAMD>(get_device(static_cast<VkDevice>(facts->device),"vkAntiLagUpdateAMD")) : nullptr;
    if (!callback) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    return rsf_core_amd_vk_bind(facts->device,reinterpret_cast<rsf_vk_function>(callback),update_device);
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" void rsf_latencyvk_device_destroyed(void* device) {
    rsf_core_amd_vk_device_destroyed(device);
}
