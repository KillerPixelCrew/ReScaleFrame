// SPDX-License-Identifier: GPL-3.0-only
#include "latency_amd_vulkan.h"
#include <rescaleframe/native_latency_vulkan_requirements.h>
#include <rescaleframe/native_cpu.h>
#include <mutex>
#include <array>
#include <cstring>
#include <vector>
#if defined(RSF_HAVE_ANTILAG_VK)
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
namespace {
std::mutex guard;
VkDevice device{};
PFN_vkAntiLagUpdateAMD update{};
uint32_t mode{},limit{};
bool configured{};
rsf::latency::AmdVulkanStatus state{};
struct Key {uint64_t frame{};bool input{},present_owed{};};
std::array<Key,128> keys{};
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
struct Call {VkDevice device;PFN_vkAntiLagUpdateAMD update;uint32_t mode,limit;};
Call snapshot() {return {device,update,mode,limit};}
void call(const Call& current,VkAntiLagStageAMD stage,uint64_t frame,bool mode_only) {
    VkAntiLagPresentationInfoAMD present{VK_STRUCTURE_TYPE_ANTI_LAG_PRESENTATION_INFO_AMD};
    present.stage=stage;present.frameIndex=frame;
    VkAntiLagDataAMD data{VK_STRUCTURE_TYPE_ANTI_LAG_DATA_AMD};
    data.mode=current.mode ? VK_ANTI_LAG_MODE_ON_AMD : VK_ANTI_LAG_MODE_OFF_AMD;
    data.maxFPS=current.mode && current.limit ? (1000000u/current.limit ? 1000000u/current.limit : 1u) : 0;
    data.pPresentationInfo=mode_only ? nullptr : &present;
    current.update(current.device,&data);
}
void clear() {
    if (update && device && state.enabled) {mode=0;call(snapshot(),VK_ANTI_LAG_STAGE_INPUT_AMD,0,true);}
    update=nullptr;device=VK_NULL_HANDLE;mode=limit=0;configured=false;keys={};state={};
}
}
#endif
extern "C" rsf_backend_result rsf_latencyvk_require_device(const rsf_latencyvk_device_query* query,rsf_latencyvk_device_requirements* out) try {
    if (!query || !out || query->struct_size<sizeof(*query) || out->struct_size<sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (query->abi_version!=1 || out->abi_version!=1) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!query->instance || !query->physical_device || !query->instance_proc) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out={sizeof(*out),1,0,nullptr,query->features};
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
    feature={VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD};
    VkPhysicalDeviceFeatures2 root{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};root.pNext=&feature;
    get_features(physical,&root);
    if (!feature.antiLag) return RSF_BACKEND_OK;
    if (auto* existing=find(query->features,feature.sType)) {
        reinterpret_cast<VkPhysicalDeviceAntiLagFeaturesAMD*>(existing)->antiLag=VK_TRUE;
    } else if (auto* root_node=find(query->features,VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2)) {
        feature.pNext=root_node->pNext;root_node->pNext=reinterpret_cast<VkBaseOutStructure*>(&feature);
    } else {feature.pNext=query->features;out->features=&feature;}
    static const char* names[]{VK_AMD_ANTI_LAG_EXTENSION_NAME};out->extensions=names;out->extension_count=1;
#endif
    return RSF_BACKEND_OK;
} catch (...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" rsf_backend_result rsf_latencyvk_device_created(const rsf_latencyvk_created_device* facts) try {
    if (!facts || facts->struct_size<sizeof(*facts) || !facts->device || !facts->create_info) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (facts->abi_version!=1) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_ANTILAG_VK)
    const rsf_latencyvk_device_query query{sizeof(query),1,facts->instance,facts->physical_device,facts->instance_proc,nullptr};
    if (!query.instance || !query.physical_device || !query.instance_proc || !amd(query)) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const auto& info=*static_cast<const VkDeviceCreateInfo*>(facts->create_info);
    const auto* enabled=reinterpret_cast<const VkPhysicalDeviceAntiLagFeaturesAMD*>(find(const_cast<void*>(info.pNext),VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD));
    if (!extension(info.enabledExtensionCount,info.ppEnabledExtensionNames,VK_AMD_ANTI_LAG_EXTENSION_NAME) || !enabled || !enabled->antiLag) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const auto get_device=function<PFN_vkGetDeviceProcAddr>(query,"vkGetDeviceProcAddr");
    const auto callback=get_device ? reinterpret_cast<PFN_vkAntiLagUpdateAMD>(get_device(static_cast<VkDevice>(facts->device),"vkAntiLagUpdateAMD")) : nullptr;
    if (!callback) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    std::lock_guard lock(guard);
    if (device && device!=facts->device) return RSF_BACKEND_ERROR_NOT_READY;
    device=static_cast<VkDevice>(facts->device);update=callback;state.available=true;
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" void rsf_latencyvk_device_destroyed(void* value) {
#if defined(RSF_HAVE_ANTILAG_VK)
    std::lock_guard lock(guard);
    if (device==value) {clear();rsf_native_latency_release_claim(2);}
#else
    (void)value;
#endif
}
namespace rsf::latency {
bool amd_vk_capable() {
#if defined(RSF_HAVE_ANTILAG_VK)
    std::lock_guard lock(guard);return state.available;
#else
    return false;
#endif
}
void amd_vk_options(uint32_t requested,uint32_t interval) {
#if defined(RSF_HAVE_ANTILAG_VK)
    Call current{};bool changed=false;
    {
        std::lock_guard lock(guard);
        if (!update || !device) return;
        const auto next=requested ? 1u : 0u;
        if (!configured || mode!=next || limit!=interval) {
            if (mode!=next) for (auto& key:keys) key.present_owed=false;
            mode=next;limit=interval;changed=true;configured=true;
        }
        state.enabled=mode!=0;current=snapshot();
    }
    if (changed) call(current,VK_ANTI_LAG_STAGE_INPUT_AMD,0,true);
#else
    (void)requested;(void)interval;
#endif
}
void amd_vk_begin(uint64_t frame) {
#if defined(RSF_HAVE_ANTILAG_VK)
    std::lock_guard lock(guard);if (frame) keys[frame%keys.size()]={frame};
#else
    (void)frame;
#endif
}
void amd_vk_input(uint64_t frame) {
#if defined(RSF_HAVE_ANTILAG_VK)
    Call current{};
    {
        std::lock_guard lock(guard);auto& key=keys[frame%keys.size()];
        if (!update || !mode || !frame || key.frame!=frame || key.input) return;
        key.input=key.present_owed=true;current=snapshot();++state.input_updates;
    }
    // The input update can block. Present markers must remain able to advance on another thread.
    call(current,VK_ANTI_LAG_STAGE_INPUT_AMD,frame,false);
#else
    (void)frame;
#endif
}
void amd_vk_present(uint64_t frame) {
#if defined(RSF_HAVE_ANTILAG_VK)
    Call current{};
    {
        std::lock_guard lock(guard);auto& key=keys[frame%keys.size()];
        if (!update || !mode || !frame || key.frame!=frame || !key.present_owed) return;
        key.present_owed=false;current=snapshot();++state.present_updates;
    }
    call(current,VK_ANTI_LAG_STAGE_PRESENT_AMD,frame,false);
#else
    (void)frame;
#endif
}
void amd_vk_cancel(uint64_t frame) {
#if defined(RSF_HAVE_ANTILAG_VK)
    std::lock_guard lock(guard);auto& key=keys[frame%keys.size()];if (key.frame==frame) key.present_owed=false;
#else
    (void)frame;
#endif
}
AmdVulkanStatus amd_vk_status() {
#if defined(RSF_HAVE_ANTILAG_VK)
    std::lock_guard lock(guard);return state;
#else
    return {};
#endif
}
void amd_vk_shutdown() {
#if defined(RSF_HAVE_ANTILAG_VK)
    std::lock_guard lock(guard);clear();
#endif
}
}
