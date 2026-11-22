// SPDX-License-Identifier: MIT
// CPU contract for the shared Vulkan sharpening owner and its native leaves.
// Fake Vulkan entry points record every call; no device is used.
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#ifdef RSF_SHARPENING_VULKAN_HEADER
#include RSF_SHARPENING_VULKAN_HEADER
#else
#include "sharpening_vulkan_support.h"
#include "../runtime/graphics/src/sharpening/options.h"
#include "../runtime/graphics/src/sharpening/rcas_spirv.h"
#include "../runtime/graphics/src/sharpening/da_rcas_spirv.h"
#include "../runtime/graphics/src/sharpening/da_das_spirv.h"
#endif
#include <cstdio>
#include <cstring>
#include <limits>
#include <map>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
void require(bool value, const char* message) { if (!value) throw std::runtime_error(message); }

struct Write { uint32_t binding; VkDescriptorType type; VkImageView view; VkImageLayout layout; VkBuffer buffer; };
struct Dispatch { VkCommandBuffer commands; VkPipeline pipeline; uint32_t x, y, z; };
struct State {
    uint64_t next = 0x10000;
    std::map<uint64_t, std::string> live;
    std::map<uint64_t, uint64_t> set_pool;
    std::vector<std::string> calls;
    std::vector<std::pair<const void*, size_t>> shaders;
    std::vector<std::string> entries;
    std::vector<VkPipeline> pipelines;
    std::vector<VkImageCreateInfo> images;
    std::vector<VkImageViewCreateInfo> views;
    std::vector<uint32_t> memory_types;
    std::vector<VkDescriptorPool> pools;
    std::vector<Write> writes;
    std::vector<VkImageMemoryBarrier> barriers;
    std::vector<Dispatch> dispatches;
    std::vector<VkImageCopy> copies;
    std::vector<std::string> logs;
    std::string fail;
    alignas(16) unsigned char mapped[256]{};
} g;

template<class T> T make(const char* kind) {
    const auto handle = g.next++;
    g.live[handle] = kind;
    return reinterpret_cast<T>(handle);
}
template<class T> void drop(T handle) {
    if (!handle) return;
    require(g.live.erase(reinterpret_cast<uint64_t>(handle)) == 1, "a released handle was live");
}
bool fails(const char* name) { g.calls.emplace_back(name); return g.fail == name; }

VKAPI_ATTR VkResult VKAPI_CALL create_shader(VkDevice, const VkShaderModuleCreateInfo* info, const VkAllocationCallbacks*, VkShaderModule* out) {
    if (fails("vkCreateShaderModule")) return VK_ERROR_OUT_OF_HOST_MEMORY;
    g.shaders.emplace_back(info->pCode, info->codeSize); *out = make<VkShaderModule>("shader"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_shader(VkDevice, VkShaderModule module, const VkAllocationCallbacks*) { drop(module); }
VKAPI_ATTR VkResult VKAPI_CALL create_set_layout(VkDevice, const VkDescriptorSetLayoutCreateInfo* info, const VkAllocationCallbacks*, VkDescriptorSetLayout* out) {
    if (fails("vkCreateDescriptorSetLayout")) return VK_ERROR_OUT_OF_HOST_MEMORY;
    require(info->bindingCount == 6, "six bindings");
    for (uint32_t i = 0; i < 6; ++i) {
        const auto expected = i == 0 ? VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER : i == 5 ? VK_DESCRIPTOR_TYPE_STORAGE_IMAGE : VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE;
        require(info->pBindings[i].binding == i && info->pBindings[i].descriptorType == expected &&
            info->pBindings[i].stageFlags == VK_SHADER_STAGE_COMPUTE_BIT && info->pBindings[i].descriptorCount == 1, "binding layout");
    }
    *out = make<VkDescriptorSetLayout>("set layout"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_set_layout(VkDevice, VkDescriptorSetLayout layout, const VkAllocationCallbacks*) { drop(layout); }
VKAPI_ATTR VkResult VKAPI_CALL create_pipeline_layout(VkDevice, const VkPipelineLayoutCreateInfo*, const VkAllocationCallbacks*, VkPipelineLayout* out) {
    if (fails("vkCreatePipelineLayout")) return VK_ERROR_OUT_OF_HOST_MEMORY;
    *out = make<VkPipelineLayout>("pipeline layout"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_pipeline_layout(VkDevice, VkPipelineLayout layout, const VkAllocationCallbacks*) { drop(layout); }
VKAPI_ATTR VkResult VKAPI_CALL create_pipelines(VkDevice, VkPipelineCache, uint32_t count, const VkComputePipelineCreateInfo* info, const VkAllocationCallbacks*, VkPipeline* out) {
    require(count == 1 && g.live.count(reinterpret_cast<uint64_t>(info->stage.module)), "one pipeline from a live module");
    if (fails("vkCreateComputePipelines")) { *out = VK_NULL_HANDLE; return VK_ERROR_OUT_OF_DEVICE_MEMORY; }
    g.entries.emplace_back(info->stage.pName); *out = make<VkPipeline>("pipeline"); g.pipelines.push_back(*out); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_pipeline(VkDevice, VkPipeline pipeline, const VkAllocationCallbacks*) { drop(pipeline); }
VKAPI_ATTR VkResult VKAPI_CALL create_pool(VkDevice, const VkDescriptorPoolCreateInfo* info, const VkAllocationCallbacks*, VkDescriptorPool* out) {
    if (fails("vkCreateDescriptorPool")) return VK_ERROR_OUT_OF_HOST_MEMORY;
    require(info->maxSets == 1 && info->poolSizeCount == 3, "one set per slot");
    *out = make<VkDescriptorPool>("pool"); g.pools.push_back(*out); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_pool(VkDevice, VkDescriptorPool pool, const VkAllocationCallbacks*) {
    for (auto it = g.set_pool.begin(); it != g.set_pool.end();)
        if (it->second == reinterpret_cast<uint64_t>(pool)) { g.live.erase(it->first); it = g.set_pool.erase(it); } else ++it;
    drop(pool);
}
VKAPI_ATTR VkResult VKAPI_CALL allocate_sets(VkDevice, const VkDescriptorSetAllocateInfo* info, VkDescriptorSet* out) {
    if (fails("vkAllocateDescriptorSets")) return VK_ERROR_OUT_OF_POOL_MEMORY;
    *out = make<VkDescriptorSet>("set"); g.set_pool[reinterpret_cast<uint64_t>(*out)] = reinterpret_cast<uint64_t>(info->descriptorPool);
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL update_sets(VkDevice, uint32_t count, const VkWriteDescriptorSet* writes, uint32_t copies, const VkCopyDescriptorSet*) {
    require(count == 6 && copies == 0, "six descriptor writes");
    for (uint32_t i = 0; i < count; ++i) {
        const auto& w = writes[i];
        require(g.live.count(reinterpret_cast<uint64_t>(w.dstSet)) == 1 && w.descriptorCount == 1, "live descriptor set");
        g.writes.push_back({w.dstBinding, w.descriptorType, w.pImageInfo ? w.pImageInfo->imageView : VK_NULL_HANDLE,
            w.pImageInfo ? w.pImageInfo->imageLayout : VK_IMAGE_LAYOUT_UNDEFINED, w.pBufferInfo ? w.pBufferInfo->buffer : VK_NULL_HANDLE});
    }
}
VKAPI_ATTR VkResult VKAPI_CALL create_buffer(VkDevice, const VkBufferCreateInfo* info, const VkAllocationCallbacks*, VkBuffer* out) {
    if (fails("vkCreateBuffer")) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    require(info->size == 128 && info->usage == VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT, "uniform block");
    *out = make<VkBuffer>("buffer"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_buffer(VkDevice, VkBuffer buffer, const VkAllocationCallbacks*) { drop(buffer); }
VKAPI_ATTR void VKAPI_CALL buffer_requirements(VkDevice, VkBuffer, VkMemoryRequirements* out) { *out = {256, 16, 0x3}; }
VKAPI_ATTR VkResult VKAPI_CALL allocate(VkDevice, const VkMemoryAllocateInfo* info, const VkAllocationCallbacks*, VkDeviceMemory* out) {
    if (fails("vkAllocateMemory")) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    g.memory_types.push_back(info->memoryTypeIndex); *out = make<VkDeviceMemory>("memory"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL free_memory(VkDevice, VkDeviceMemory memory, const VkAllocationCallbacks*) { drop(memory); }
VKAPI_ATTR VkResult VKAPI_CALL bind_buffer(VkDevice, VkBuffer, VkDeviceMemory, VkDeviceSize) { return VK_SUCCESS; }
VKAPI_ATTR VkResult VKAPI_CALL map_memory(VkDevice, VkDeviceMemory, VkDeviceSize, VkDeviceSize, VkMemoryMapFlags, void** out) { *out = g.mapped; return VK_SUCCESS; }
VKAPI_ATTR void VKAPI_CALL unmap(VkDevice, VkDeviceMemory memory) { require(g.live.count(reinterpret_cast<uint64_t>(memory)) == 1, "unmap before free"); }
VKAPI_ATTR VkResult VKAPI_CALL create_image(VkDevice, const VkImageCreateInfo* info, const VkAllocationCallbacks*, VkImage* out) {
    g.images.push_back(*info);
    if (fails("vkCreateImage")) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    *out = make<VkImage>("image"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_image(VkDevice, VkImage image, const VkAllocationCallbacks*) { drop(image); }
VKAPI_ATTR void VKAPI_CALL image_requirements(VkDevice, VkImage, VkMemoryRequirements* out) { *out = {4096, 256, 0x3}; }
VKAPI_ATTR VkResult VKAPI_CALL bind_image(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) { return VK_SUCCESS; }
VKAPI_ATTR VkResult VKAPI_CALL create_view(VkDevice, const VkImageViewCreateInfo* info, const VkAllocationCallbacks*, VkImageView* out) {
    g.views.push_back(*info);
    if (fails("vkCreateImageView") || (g.fail == "depth view" && info->format == VK_FORMAT_D32_SFLOAT)) return VK_ERROR_OUT_OF_HOST_MEMORY;
    *out = make<VkImageView>("view"); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_view(VkDevice, VkImageView view, const VkAllocationCallbacks*) { drop(view); }
VKAPI_ATTR void VKAPI_CALL barrier(VkCommandBuffer, VkPipelineStageFlags, VkPipelineStageFlags, VkDependencyFlags, uint32_t, const VkMemoryBarrier*, uint32_t, const VkBufferMemoryBarrier*, uint32_t count, const VkImageMemoryBarrier* images) {
    for (uint32_t i = 0; i < count; ++i) g.barriers.push_back(images[i]);
}
VkPipeline bound_pipeline = VK_NULL_HANDLE;
VKAPI_ATTR void VKAPI_CALL bind_pipeline(VkCommandBuffer, VkPipelineBindPoint point, VkPipeline pipeline) {
    require(point == VK_PIPELINE_BIND_POINT_COMPUTE, "compute pipeline"); bound_pipeline = pipeline;
}
VKAPI_ATTR void VKAPI_CALL bind_sets(VkCommandBuffer, VkPipelineBindPoint, VkPipelineLayout, uint32_t, uint32_t count, const VkDescriptorSet*, uint32_t, const uint32_t*) {
    require(count == 1, "one descriptor set");
}
VKAPI_ATTR void VKAPI_CALL dispatch(VkCommandBuffer commands, uint32_t x, uint32_t y, uint32_t z) { g.dispatches.push_back({commands, bound_pipeline, x, y, z}); }
VKAPI_ATTR void VKAPI_CALL copy_image(VkCommandBuffer, VkImage, VkImageLayout, VkImage, VkImageLayout, uint32_t count, const VkImageCopy* regions) {
    require(count == 1, "one copy"); g.copies.push_back(regions[0]);
}
VKAPI_ATTR void VKAPI_CALL memory_properties(VkPhysicalDevice, VkPhysicalDeviceMemoryProperties* out) {
    *out = {};
    out->memoryTypeCount = 2;
    out->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    out->memoryTypes[1].propertyFlags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
}

struct Entry { const char* name; PFN_vkVoidFunction function; };
#define RSF_FAKE(name, function) {name, reinterpret_cast<PFN_vkVoidFunction>(function)}
const Entry device_entries[] = {
    RSF_FAKE("vkCreateShaderModule", create_shader), RSF_FAKE("vkDestroyShaderModule", destroy_shader),
    RSF_FAKE("vkCreateDescriptorSetLayout", create_set_layout), RSF_FAKE("vkDestroyDescriptorSetLayout", destroy_set_layout),
    RSF_FAKE("vkCreatePipelineLayout", create_pipeline_layout), RSF_FAKE("vkDestroyPipelineLayout", destroy_pipeline_layout),
    RSF_FAKE("vkCreateComputePipelines", create_pipelines), RSF_FAKE("vkDestroyPipeline", destroy_pipeline),
    RSF_FAKE("vkCreateDescriptorPool", create_pool), RSF_FAKE("vkDestroyDescriptorPool", destroy_pool),
    RSF_FAKE("vkAllocateDescriptorSets", allocate_sets), RSF_FAKE("vkUpdateDescriptorSets", update_sets),
    RSF_FAKE("vkCreateBuffer", create_buffer), RSF_FAKE("vkDestroyBuffer", destroy_buffer),
    RSF_FAKE("vkGetBufferMemoryRequirements", buffer_requirements), RSF_FAKE("vkAllocateMemory", allocate),
    RSF_FAKE("vkFreeMemory", free_memory), RSF_FAKE("vkBindBufferMemory", bind_buffer),
    RSF_FAKE("vkMapMemory", map_memory), RSF_FAKE("vkUnmapMemory", unmap),
    RSF_FAKE("vkCreateImage", create_image), RSF_FAKE("vkDestroyImage", destroy_image),
    RSF_FAKE("vkGetImageMemoryRequirements", image_requirements), RSF_FAKE("vkBindImageMemory", bind_image),
    RSF_FAKE("vkCreateImageView", create_view), RSF_FAKE("vkDestroyImageView", destroy_view),
    RSF_FAKE("vkCmdPipelineBarrier", barrier), RSF_FAKE("vkCmdBindPipeline", bind_pipeline),
    RSF_FAKE("vkCmdBindDescriptorSets", bind_sets), RSF_FAKE("vkCmdDispatch", dispatch),
    RSF_FAKE("vkCmdCopyImage", copy_image),
};
#undef RSF_FAKE
rsf_vk_function device_proc(void*, const char* name) {
    for (const auto& entry : device_entries) if (!std::strcmp(entry.name, name)) return reinterpret_cast<rsf_vk_function>(entry.function);
    return nullptr;
}
rsf_vk_function instance_proc(void*, const char* name) {
    return std::strcmp(name, "vkGetPhysicalDeviceMemoryProperties") ? nullptr : reinterpret_cast<rsf_vk_function>(memory_properties);
}
void capture_log(void* user, const char* message) { static_cast<std::vector<std::string>*>(user)->emplace_back(message); }

template<class T> T fake(uint64_t value) { return reinterpret_cast<T>(value); }
const VkImage output = fake<VkImage>(0x51), motion = fake<VkImage>(0x52), depth = fake<VkImage>(0x53), exposure = fake<VkImage>(0x54);

struct Fixture {
    rsf::sharpening::Vulkan pass;
    rsf_srvk_setup setup{};
    rsf_frame_record record{};
    std::vector<std::string> logs;
    Fixture() {
        g = State{};
        setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
        setup.instance = fake<void*>(0x11); setup.physical_device = fake<void*>(0x12); setup.device = fake<void*>(0x13);
        setup.instance_proc = instance_proc; setup.device_proc = device_proc;
        record.output_width = 33; record.output_height = 17;
        record.camera.near_plane = 0.1f; record.camera.far_plane = 1000; record.camera.depth_inverted = 1;
    }
    rsf_srvk_image image(VkImage value, VkFormat format, uint32_t width, uint32_t height) const {
        return {setup.device, reinterpret_cast<uint64_t>(value), uint32_t(format), width, height, uint32_t(VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL), 0};
    }
    rsf_srvk_frame frame(uint32_t mode, bool with_depth = true) {
        rsf_srvk_frame f{};
        f.struct_size = sizeof(f); f.abi_version = RSF_SRVK_ABI_VERSION; f.record = &record;
        f.motion = image(motion, VK_FORMAT_R16G16_SFLOAT, 33, 17); f.motion_scale_x = f.motion_scale_y = 1;
        if (with_depth) f.depth = image(depth, VK_FORMAT_D32_SFLOAT, 33, 17);
        f.pre_exposure = 2;
        f.sharpening = rsf_sharpening_defaults(); f.sharpening.mode = mode; f.sharpening.sharpness = .5f;
        f.log = capture_log; f.log_user = &logs;
        return f;
    }
    bool run(uintptr_t commands, const rsf_srvk_frame& f) {
        return pass.run(setup, fake<void*>(commands), f, output);
    }
    size_t live(const char* kind) const {
        size_t count = 0; for (const auto& [handle, name] : g.live) count += name == kind; return count;
    }
};

void disabled_and_refused_work() {
    rsf::sharpening::Vulkan disabled; rsf_srvk_frame empty{};
    require(disabled.run({}, nullptr, empty, VK_NULL_HANDLE) && disabled.dispatches() == 0, "Off requires no device and records no work");
    Fixture f;
    auto off = f.frame(0); off.sharpening.struct_size = 0;
    require(f.run(1, off), "defaults are Off");
    require(f.run(1, f.frame(4)) && f.run(1, f.frame(9)), "Vendor and unknown modes leave the output");
    auto zero = f.frame(1); zero.sharpening.sharpness = 0; require(f.run(1, zero), "zero strength is a no-op");
    auto refused = [&](auto edit, const char* message) { auto frame = f.frame(1); edit(frame); require(!f.run(1, frame), message); };
    refused([](rsf_srvk_frame& x) { x.record = nullptr; }, "frame record required");
    refused([](rsf_srvk_frame& x) { x.motion.layout = VK_IMAGE_LAYOUT_GENERAL; }, "motion must be shader-read");
    refused([](rsf_srvk_frame& x) { x.depth.device = nullptr; }, "depth device must match");
    refused([](rsf_srvk_frame& x) { x.depth.width = 0; }, "depth must have an extent");
    refused([&](rsf_srvk_frame& x) { x.exposure = f.image(exposure, VK_FORMAT_R16_SFLOAT, 2, 1); }, "exposure must be 1x1");
    refused([&](rsf_srvk_frame& x) { x.exposure = f.image(exposure, VK_FORMAT_R16G16B16A16_SFLOAT, 1, 1); }, "exposure must be one float channel");
    require(!f.pass.run(f.setup, nullptr, f.frame(1), output), "commands required");
    require(!f.pass.run(f.setup, fake<void*>(1), f.frame(1), VK_NULL_HANDLE), "output required");
    require(g.calls.empty() && g.live.empty() && f.pass.dispatches() == 0, "refusals precede any Vulkan call");
}

void sanitized_options_activate_the_pass() {
    Fixture f;
    auto frame = f.frame(1);
    frame.sharpening.sharpness = std::numeric_limits<float>::quiet_NaN();
    require(f.run(1, frame) && f.pass.dispatches() == 1, "non-finite sharpness uses the active default");
    rsf::sharpening::Constants constants{};
    std::memcpy(&constants, g.mapped, sizeof(constants));
    require(constants.sharpness == .4f, "default strength reaches the shader");
    frame.sharpening.sharpness = 0;
    frame.sharpening.mas = 1;
    frame.sharpening.motion_sharpness = -.2f;
    require(f.run(2, frame) && f.pass.dispatches() == 1, "negative MAS with zero strength is inactive");
    frame.sharpening.motion_sharpness = .2f;
    require(f.run(2, frame) && f.pass.dispatches() == 2, "positive MAS alone activates the pass");
}

void zero_extent_is_refused() {
    Fixture f; f.record.output_height = 0;
    require(!f.run(1, f.frame(1)), "zero output extent refused");
    for (const auto& image : g.images) require(image.extent.width && image.extent.height, "no zero-extent image (VUID-VkImageCreateInfo-extent-00944)");
    f.pass.clear(); require(g.live.empty(), "clear after refusal");
}

void records_exact_pass() {
    Fixture f;
    auto frame = f.frame(2); frame.exposure = f.image(exposure, VK_FORMAT_R16_SFLOAT, 1, 1);
    require(f.run(7, frame), "depth RCAS recorded");
    require(g.shaders.size() == 3, "three shader modules");
    require(g.shaders[0].second == sizeof(rsf::sharpening::rcas_spirv) && !std::memcmp(g.shaders[0].first, rsf::sharpening::rcas_spirv, sizeof(rsf::sharpening::rcas_spirv)) &&
        g.shaders[1].second == sizeof(rsf::sharpening::da_rcas_spirv) && !std::memcmp(g.shaders[1].first, rsf::sharpening::da_rcas_spirv, sizeof(rsf::sharpening::da_rcas_spirv)) &&
        g.shaders[2].second == sizeof(rsf::sharpening::da_das_spirv) && !std::memcmp(g.shaders[2].first, rsf::sharpening::da_das_spirv, sizeof(rsf::sharpening::da_das_spirv)), "exact SPIR-V in mode order");
    require(f.live("shader") == 0, "shader modules released after pipeline creation");
    for (const auto& entry : g.entries) require(entry == "CSMain", "CSMain entry point");
    require(g.memory_types.size() == 2 && g.memory_types[0] == 1 && g.memory_types[1] == 0, "host-coherent constants, device-local scratch");
    require(g.images.size() == 1 && g.images[0].format == VK_FORMAT_R16G16B16A16_SFLOAT && g.images[0].extent.width == 33 &&
        g.images[0].extent.height == 17 && g.images[0].usage == (VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT) &&
        g.images[0].tiling == VK_IMAGE_TILING_OPTIMAL && g.images[0].mipLevels == 1 && g.images[0].arrayLayers == 1, "RGBA16F scratch");
    std::map<uint64_t, VkImageViewCreateInfo> views;
    for (const auto& view : g.views) views[reinterpret_cast<uint64_t>(view.image)] = view;
    require(views.size() == 5, "scratch, output and three input views");
    require(views[reinterpret_cast<uint64_t>(output)].format == VK_FORMAT_R16G16B16A16_SFLOAT, "output viewed as RGBA16F");
    require(views[reinterpret_cast<uint64_t>(depth)].subresourceRange.aspectMask == VK_IMAGE_ASPECT_DEPTH_BIT, "D32 depth uses the depth aspect");
    require(views[reinterpret_cast<uint64_t>(motion)].format == VK_FORMAT_R16G16_SFLOAT &&
        views[reinterpret_cast<uint64_t>(motion)].subresourceRange.aspectMask == VK_IMAGE_ASPECT_COLOR_BIT, "motion keeps its format");
    require(views[reinterpret_cast<uint64_t>(exposure)].format == VK_FORMAT_R16_SFLOAT, "R16 exposure accepted");
    require(g.writes.size() == 6, "one descriptor update");
    for (uint32_t i = 0; i < 6; ++i) {
        const auto& w = g.writes[i];
        require(w.binding == i, "bindings in order");
        if (i == 0) require(w.type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER && w.buffer, "constants");
        else if (i == 5) require(w.type == VK_DESCRIPTOR_TYPE_STORAGE_IMAGE && w.layout == VK_IMAGE_LAYOUT_GENERAL, "scratch storage");
        else require(w.type == VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE && w.layout == VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL && w.view, "sampled inputs");
    }
    require(g.dispatches.size() == 1 && g.dispatches[0].x == 3 && g.dispatches[0].y == 2 && g.dispatches[0].z == 1 &&
        g.dispatches[0].pipeline == g.pipelines[1] && g.dispatches[0].commands == fake<VkCommandBuffer>(7), "depth RCAS dispatch");
    const VkImageLayout layouts[6][2] = {
        {VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL}, {VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL},
        {VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL}, {VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL},
        {VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL}, {VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL}};
    require(g.barriers.size() == 6, "six image barriers");
    for (uint32_t i = 0; i < 6; ++i)
        require(g.barriers[i].oldLayout == layouts[i][0] && g.barriers[i].newLayout == layouts[i][1] &&
            g.barriers[i].image == (i == 0 || i == 3 || i == 4 ? output : g.barriers[1].image), "barrier sequence");
    require(g.copies.size() == 1 && g.copies[0].extent.width == 33 && g.copies[0].extent.height == 17, "scratch copy");
    rsf::sharpening::Constants constants{}; std::memcpy(&constants, g.mapped, sizeof(constants));
    require(constants.sharpness == 1 && constants.exposure == .5f && constants.width == 33 && constants.height == 17 &&
        constants.exposure_texture == 1 && constants.known_planes == 1 && constants.depth_inverted == 1, "constants");

    // Without depth, depth RCAS records RCAS and binds the output view for the absent inputs.
    g.writes.clear(); g.barriers.clear();
    require(f.run(8, f.frame(2, false)), "fallback recorded");
    require(g.dispatches.back().pipeline == g.pipelines[0], "RCAS fallback pipeline");
    require(g.writes[3].view == g.writes[1].view && g.writes[4].view == g.writes[1].view && g.writes[2].view != g.writes[1].view, "absent inputs bind the output");
    require(g.pools.size() == 2, "in-flight recordings use separate slots");
    std::memcpy(&constants, g.mapped, sizeof(constants)); require(constants.sharpness == .5f, "RCAS strength is not doubled");

    const auto views_before = f.live("view");
    f.pass.retire(fake<void*>(7));
    require(f.live("view") == views_before - 4, "retire releases the recording's four views");
    f.pass.retire(nullptr); f.pass.retire(fake<void*>(99));
    require(f.live("view") == views_before - 4, "unknown retirement is ignored");
    g.barriers.clear(); g.writes.clear();
    require(f.run(9, f.frame(3)), "depth DAS recorded");
    require(g.pools.size() == 2 && g.dispatches.back().pipeline == g.pipelines[2], "retired slot reused for depth DAS");
    require(g.barriers[1].oldLayout == VK_IMAGE_LAYOUT_GENERAL, "initialized scratch keeps GENERAL");
    require(f.pass.dispatches() == 3 && f.logs.empty(), "three dispatches");
    f.pass.clear();
    require(g.live.empty(), "clear releases every Vulkan object");
}

void bounded_slots_and_single_report() {
    Fixture f; const auto frame = f.frame(1);
    for (uintptr_t commands = 1; commands <= 64; ++commands) require(f.run(commands, frame), "64 in-flight recordings");
    require(!f.run(65, frame) && !f.run(66, frame) && f.logs.size() == 1 &&
        f.logs[0] == "Vulkan sharpening unavailable; retaining vendor output", "one report");
    f.pass.retire(fake<void*>(5));
    require(f.run(65, frame) && f.pass.dispatches() == 65 && g.pools.size() == 64, "retired slot reused");
    f.pass.clear(); require(g.live.empty(), "clear releases 64 slots");
}

void failures_release_or_latch() {
    {
        Fixture f; g.fail = "vkCreateComputePipelines";
        require(!f.run(1, f.frame(1)), "pipeline failure refused");
        g.fail.clear(); const auto modules = g.shaders.size();
        require(!f.run(1, f.frame(1)) && g.shaders.size() == modules, "failed binding latched until clear");
        f.pass.clear(); require(g.live.empty(), "partial binding released");
        require(f.run(1, f.frame(1)), "rebinding after clear");
        f.pass.clear();
    }
    {
        Fixture f; g.fail = "vkCreateImage";
        require(!f.run(1, f.frame(1)), "scratch failure refused");
        require(f.live("pool") == 0 && f.live("buffer") == 0 && f.live("memory") == 0, "failed slot released");
        g.fail = "depth view";
        require(!f.run(2, f.frame(2)), "view failure refused");
        g.fail.clear(); f.pass.retire(fake<void*>(2));
        require(f.run(3, f.frame(2)) && f.pass.dispatches() == 1, "slot usable after refusals");
        f.pass.clear(); require(g.live.empty(), "no leak after refusals");
    }
#ifndef RSF_SHARPENING_VULKAN_HEADER
    // The baseline header called a null instance_proc here.
    {
        Fixture f; f.setup.instance_proc = nullptr;
        require(!f.run(1, f.frame(1)) && g.calls.empty() && f.logs.size() == 1, "missing dispatch is refused");
        f.pass.clear();
    }
#endif
}
}

int main() try {
    disabled_and_refused_work();
    sanitized_options_activate_the_pass();
    zero_extent_is_refused();
    records_exact_pass();
    bounded_slots_and_single_report();
    failures_release_or_latch();
    std::printf("sharpening_vulkan_cpu: PASS\n");
    return 0;
} catch (const std::exception& error) {
    std::printf("FAIL: %s\n", error.what());
    return 1;
}
