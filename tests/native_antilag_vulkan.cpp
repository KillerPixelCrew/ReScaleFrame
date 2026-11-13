// SPDX-License-Identifier: GPL-3.0-only
// Stubbed Vulkan function pointers. No Vulkan loader, device or AMD driver is used.
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include <rescaleframe/native_latency_vulkan_requirements.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/frame_generation.h>
#include <array>
#include <vector>
#include <mutex>
#include <condition_variable>
#include <thread>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
namespace {
int failures{};
#define CHECK(x) do {if (!(x)) {std::printf("FAIL %d: %s\n",__LINE__,#x);++failures;}} while(false)
const auto instance=reinterpret_cast<VkInstance>(uintptr_t(1));
const auto physical=reinterpret_cast<VkPhysicalDevice>(uintptr_t(2));
const auto device=reinterpret_cast<VkDevice>(uintptr_t(3));
uint32_t vendor=0x1002;bool extension_available=true,feature_available=true,function_available=true;
struct Mark {uint64_t frame;uint32_t mode,fps,stage;};
std::vector<Mark> marks;
std::mutex records;
std::condition_variable changed;
bool blocked{},released{};uint64_t blocking_frame{};
void VKAPI_PTR properties(VkPhysicalDevice,VkPhysicalDeviceProperties* value) {value->vendorID=vendor;}
VkResult VKAPI_PTR extensions(VkPhysicalDevice,const char*,uint32_t* count,VkExtensionProperties* values) {
    const auto wanted=extension_available ? 1u : 0u;
    if (!values) {*count=wanted;return VK_SUCCESS;}
    if (wanted && *count) std::strcpy(values[0].extensionName,VK_AMD_ANTI_LAG_EXTENSION_NAME);
    *count=wanted;return VK_SUCCESS;
}
void VKAPI_PTR features(VkPhysicalDevice,VkPhysicalDeviceFeatures2* root) {
    auto* feature=static_cast<VkPhysicalDeviceAntiLagFeaturesAMD*>(root->pNext);
    CHECK(feature->sType==VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD);
    feature->antiLag=feature_available;
}
void VKAPI_PTR update(VkDevice target,const VkAntiLagDataAMD* data) {
    CHECK(target==device && data && data->sType==VK_STRUCTURE_TYPE_ANTI_LAG_DATA_AMD);
    const auto* info=data->pPresentationInfo;
    std::unique_lock lock(records);
    marks.push_back({info ? info->frameIndex : 0,uint32_t(data->mode),data->maxFPS,info ? uint32_t(info->stage) : UINT32_MAX});
    if (info && info->stage==VK_ANTI_LAG_STAGE_INPUT_AMD && info->frameIndex==blocking_frame) {
        blocked=true;changed.notify_all();
        if (!changed.wait_for(lock,std::chrono::seconds(3),[] {return released;})) ++failures;
    }
}
PFN_vkVoidFunction VKAPI_PTR device_proc(VkDevice,const char* name) {
    return function_available && std::strcmp(name,"vkAntiLagUpdateAMD")==0 ? reinterpret_cast<PFN_vkVoidFunction>(update) : nullptr;
}
rsf_vk_function instance_proc(void*,const char* name) {
    if (!std::strcmp(name,"vkGetPhysicalDeviceProperties")) return reinterpret_cast<rsf_vk_function>(properties);
    if (!std::strcmp(name,"vkGetPhysicalDeviceFeatures2")) return reinterpret_cast<rsf_vk_function>(features);
    if (!std::strcmp(name,"vkEnumerateDeviceExtensionProperties")) return reinterpret_cast<rsf_vk_function>(extensions);
    if (!std::strcmp(name,"vkGetDeviceProcAddr")) return reinterpret_cast<rsf_vk_function>(device_proc);
    return nullptr;
}
rsf_game_cpu_event fact(uint64_t source,uint32_t stage) {
    static uint64_t timestamp=100;
    return {sizeof(rsf_game_cpu_event),stage,77,source,++timestamp,1000000,1,0,0,1,RSF_GAME_CPU_INPUT_KEYED};
}
void cpu(uint64_t source,uint32_t stage) {auto event=fact(source,stage);rsf_latencyvk_cpu(&event);}
size_t count(uint32_t stage,uint64_t source) {
    std::lock_guard lock(records);size_t result=0;for (const auto& mark:marks) if (mark.stage==stage && mark.frame==source) ++result;return result;
}
}
// No XeLL context is present in this contract. Its separate contract covers owner precedence.
extern "C" rsf_backend_result rsf_fgvk_backend(const rsf_fgvk*,uint32_t*) {return RSF_BACKEND_ERROR_NOT_READY;}
extern "C" rsf_backend_result rsf_fgvk_latency_options(rsf_fgvk*,uint32_t,uint32_t) {return RSF_BACKEND_ERROR_NOT_READY;}
extern "C" rsf_backend_result rsf_fgvk_latency_sleep(rsf_fgvk*,uint64_t) {return RSF_BACKEND_ERROR_NOT_READY;}
extern "C" rsf_backend_result rsf_fgvk_latency_marker(rsf_fgvk*,uint64_t,uint32_t) {return RSF_BACKEND_ERROR_NOT_READY;}
extern "C" rsf_backend_result rsf_fgvk_latency_get_status(rsf_fgvk*,rsf_fgvk_latency_status*) {return RSF_BACKEND_ERROR_NOT_READY;}
int main() {
    rsf_latencyvk_device_query query{sizeof(query),1,instance,physical,instance_proc,nullptr};
    rsf_latencyvk_device_requirements required{sizeof(required),1};
    CHECK(rsf_latencyvk_require_device(nullptr,&required)==RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    auto short_query=query;--short_query.struct_size;
    CHECK(rsf_latencyvk_require_device(&short_query,&required)==RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    short_query=query;short_query.abi_version=99;
    CHECK(rsf_latencyvk_require_device(&short_query,&required)==RSF_BACKEND_ERROR_ABI_MISMATCH);
    vendor=0x10de;CHECK(rsf_latencyvk_require_device(&query,&required)==0 && !required.extension_count);
    vendor=0x8086;CHECK(rsf_latencyvk_require_device(&query,&required)==0 && !required.extension_count);
    vendor=0x1002;extension_available=false;CHECK(rsf_latencyvk_require_device(&query,&required)==0 && !required.extension_count);
    extension_available=true;feature_available=false;CHECK(rsf_latencyvk_require_device(&query,&required)==0 && !required.extension_count);
    feature_available=true;CHECK(rsf_latencyvk_require_device(&query,&required)==0 && required.extension_count==1);
    CHECK(std::strcmp(required.extensions[0],VK_AMD_ANTI_LAG_EXTENSION_NAME)==0);
    auto* feature=static_cast<VkPhysicalDeviceAntiLagFeaturesAMD*>(required.features);
    CHECK(feature && feature->antiLag && !feature->pNext);
    VkDeviceCreateInfo info{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};info.pNext=feature;
    info.enabledExtensionCount=required.extension_count;info.ppEnabledExtensionNames=required.extensions;
    rsf_latencyvk_created_device created{sizeof(created),1,instance,physical,device,instance_proc,&info};
    info.enabledExtensionCount=0;CHECK(rsf_latencyvk_device_created(&created)==RSF_BACKEND_ERROR_NOT_SUPPORTED);
    info.enabledExtensionCount=1;feature->antiLag=false;CHECK(rsf_latencyvk_device_created(&created)==RSF_BACKEND_ERROR_NOT_SUPPORTED);
    feature->antiLag=true;function_available=false;CHECK(rsf_latencyvk_device_created(&created)==RSF_BACKEND_ERROR_NOT_SUPPORTED);
    function_available=true;CHECK(rsf_latencyvk_device_created(&created)==0);
    CHECK(rsf_native_latency_claim(1));
    CHECK(rsf_latencyvk_options(1,10000)==RSF_BACKEND_ERROR_NOT_SUPPORTED);
    cpu(1,0);cpu(1,1);cpu(1,2);cpu(1,3);cpu(1,4);
    CHECK(marks.empty()); // another orchestrator owns pacing
    rsf_native_latency_release_claim(1);
    CHECK(rsf_latencyvk_options(2,10000)==0);
    const uint64_t first=UINT64_C(0x100000032),next=first+1;
    cpu(first,0);cpu(first,1);cpu(first,1);cpu(first,2);cpu(first,3);cpu(first,4);
    CHECK(count(VK_ANTI_LAG_STAGE_INPUT_AMD,first)==1);
    {std::lock_guard lock(records);CHECK(marks.back().mode==VK_ANTI_LAG_MODE_ON_AMD && marks.back().fps==100);}
    cpu(next,0);blocking_frame=next;
    auto input=fact(next,1);
    std::thread sleeper([&] {rsf_latencyvk_cpu(&input);});
    {std::unique_lock lock(records);CHECK(changed.wait_for(lock,std::chrono::seconds(2),[] {return blocked;}));}
    rsf_latencyvk_render(first);rsf_latencyvk_present(1);rsf_latencyvk_present(1);rsf_latencyvk_present(0);
    CHECK(count(VK_ANTI_LAG_STAGE_PRESENT_AMD,first)==1 && !count(VK_ANTI_LAG_STAGE_PRESENT_AMD,next));
    {std::lock_guard lock(records);released=true;}changed.notify_all();sleeper.join();
    cpu(next,2);cpu(next,3);auto skipped=fact(next,4);skipped.flags|=RSF_GAME_CPU_RENDER_SKIPPED;skipped.render_expected=0;rsf_latencyvk_cpu(&skipped);
    rsf_latencyvk_render(next);rsf_latencyvk_present(1);rsf_latencyvk_present(0);
    CHECK(!count(VK_ANTI_LAG_STAGE_PRESENT_AMD,next));
    CHECK(rsf_latencyvk_options(0,10000)==0);
    cpu(next+1,0);cpu(next+1,1);cpu(next+1,2);cpu(next+1,3);cpu(next+1,4);
    rsf_latencyvk_render(next+1);rsf_latencyvk_present(1);rsf_latencyvk_present(0);
    CHECK(!count(VK_ANTI_LAG_STAGE_INPUT_AMD,next+1) && !count(VK_ANTI_LAG_STAGE_PRESENT_AMD,next+1));
    {std::lock_guard lock(records);CHECK(marks.back().mode==VK_ANTI_LAG_MODE_OFF_AMD && marks.back().fps==0 && marks.back().stage==UINT32_MAX);}
    rsf_latencyvk_status status{sizeof(status),RSF_LATENCYVK_ABI_VERSION};
    CHECK(rsf_latencyvk_get_status(&status)==0 && status.owner==RSF_PACING_ANTILAG_VULKAN && !status.mode && status.sleep_calls==2);
    rsf_latencyvk_device_destroyed(device);CHECK(!rsf_native_latency_has_claim(2));
    CHECK(rsf_latencyvk_options(1,0)==RSF_BACKEND_ERROR_NOT_SUPPORTED);
    rsf_latencyvk_shutdown();rsf_native_cpu_release();
    std::printf("%d failures; AMD Vulkan requirements, same-index pairing, no duplicate sleep, skip/Off and unlocked Present verified with stubs\n",failures);
    return failures ? 1 : 0;
}
