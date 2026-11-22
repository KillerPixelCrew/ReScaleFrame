// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <vulkan/vulkan.h>
#include "sharpening_pass_options.h"
#include "sharpening_vulkan_core.h"
#include "vulkan_fg_resources_native.h"
#include "../../graphics/src/sharpening/rcas_spirv.h"
#include "../../graphics/src/sharpening/da_rcas_spirv.h"
#include "../../graphics/src/sharpening/da_das_spirv.h"
#include <windows.h>
#include <cstddef>
#include <cstring>
#include <new>
namespace rsf::sharpening {
static_assert(sizeof(rsf_sharpenvk_uniform) == 40 && offsetof(rsf_sharpenvk_uniform, mapped) == 32);
static_assert(sizeof(rsf_sharpenvk_layouts) == 16);
static_assert(sizeof(rsf_sharpenvk_facts) == 56 && offsetof(rsf_sharpenvk_facts, near_plane) == 36);
static_assert(sizeof(rsf_sharpenvk_pass) == 112 && offsetof(rsf_sharpenvk_pass, width) == 88);
static_assert(sizeof(rsf_sharpenvk_platform) == 14 * sizeof(void*));
static_assert(sizeof(Constants) == 128);

// Typed Vulkan leaves for the Rust owner in vulkan_sharpening.rs. They never
// decide admission, slot reuse or lifetime.
namespace vulkan_leaf {
struct Device {
    rsf::vkfg::Device images;
    VkDevice device = VK_NULL_HANDLE;
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkCreateShaderModule CreateShaderModule=nullptr;
    PFN_vkDestroyShaderModule DestroyShaderModule=nullptr;
    PFN_vkCreateDescriptorSetLayout CreateDescriptorSetLayout=nullptr;
    PFN_vkDestroyDescriptorSetLayout DestroyDescriptorSetLayout=nullptr;
    PFN_vkCreatePipelineLayout CreatePipelineLayout=nullptr;
    PFN_vkDestroyPipelineLayout DestroyPipelineLayout=nullptr;
    PFN_vkCreateComputePipelines CreateComputePipelines=nullptr;
    PFN_vkDestroyPipeline DestroyPipeline=nullptr;
    PFN_vkCreateDescriptorPool CreateDescriptorPool=nullptr;
    PFN_vkDestroyDescriptorPool DestroyDescriptorPool=nullptr;
    PFN_vkAllocateDescriptorSets AllocateDescriptorSets=nullptr;
    PFN_vkUpdateDescriptorSets UpdateDescriptorSets=nullptr;
    PFN_vkCreateBuffer CreateBuffer=nullptr;
    PFN_vkDestroyBuffer DestroyBuffer=nullptr;
    PFN_vkGetBufferMemoryRequirements GetBufferMemoryRequirements=nullptr;
    PFN_vkBindBufferMemory BindBufferMemory=nullptr;
    PFN_vkMapMemory MapMemory=nullptr;
    PFN_vkUnmapMemory UnmapMemory=nullptr;
    PFN_vkCmdPipelineBarrier CmdPipelineBarrier=nullptr;
    PFN_vkCmdBindPipeline CmdBindPipeline=nullptr;
    PFN_vkCmdBindDescriptorSets CmdBindDescriptorSets=nullptr;
    PFN_vkCmdDispatch CmdDispatch=nullptr;
    PFN_vkCmdCopyImage CmdCopyImage=nullptr;
};
template<class T> T* handle(uint64_t value) { return reinterpret_cast<T*>(value); }
template<class T> uint64_t value(T* handle) { return reinterpret_cast<uint64_t>(handle); }
inline constexpr VkDescriptorType binding_type(uint32_t i) {
    return i==0?VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER:i==5?VK_DESCRIPTOR_TYPE_STORAGE_IMAGE:VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE;
}
inline int32_t open(const rsf_srvk_setup* setup, void** out) {
    *out = nullptr;
    if (!setup->device || !setup->device_proc || !setup->instance_proc) return VK_ERROR_INITIALIZATION_FAILED;
    auto properties=reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup->instance_proc(setup->instance,"vkGetPhysicalDeviceMemoryProperties"));
    auto* native = new (std::nothrow) Device;
    if (!native) return VK_ERROR_OUT_OF_HOST_MEMORY;
    native->device = static_cast<VkDevice>(setup->device);
    bool loaded = native->images.load_images(*setup, true);
#define RSF_SHARPEN_LOAD(name) native->name=reinterpret_cast<PFN_vk##name>(setup->device_proc(setup->device,"vk" #name)); loaded = loaded && native->name
    RSF_SHARPEN_LOAD(CreateShaderModule); RSF_SHARPEN_LOAD(DestroyShaderModule);
    RSF_SHARPEN_LOAD(CreateDescriptorSetLayout); RSF_SHARPEN_LOAD(DestroyDescriptorSetLayout);
    RSF_SHARPEN_LOAD(CreatePipelineLayout); RSF_SHARPEN_LOAD(DestroyPipelineLayout);
    RSF_SHARPEN_LOAD(CreateComputePipelines); RSF_SHARPEN_LOAD(DestroyPipeline);
    RSF_SHARPEN_LOAD(CreateDescriptorPool); RSF_SHARPEN_LOAD(DestroyDescriptorPool);
    RSF_SHARPEN_LOAD(AllocateDescriptorSets); RSF_SHARPEN_LOAD(UpdateDescriptorSets);
    RSF_SHARPEN_LOAD(CreateBuffer); RSF_SHARPEN_LOAD(DestroyBuffer);
    RSF_SHARPEN_LOAD(GetBufferMemoryRequirements); RSF_SHARPEN_LOAD(BindBufferMemory);
    RSF_SHARPEN_LOAD(MapMemory); RSF_SHARPEN_LOAD(UnmapMemory);
    RSF_SHARPEN_LOAD(CmdPipelineBarrier); RSF_SHARPEN_LOAD(CmdBindPipeline);
    RSF_SHARPEN_LOAD(CmdBindDescriptorSets); RSF_SHARPEN_LOAD(CmdDispatch); RSF_SHARPEN_LOAD(CmdCopyImage);
#undef RSF_SHARPEN_LOAD
    if (!loaded || !properties) { delete native; return VK_ERROR_INITIALIZATION_FAILED; }
    properties(static_cast<VkPhysicalDevice>(setup->physical_device),&native->memory);
    native->images.memory_types(native->memory);
    *out = native;
    return VK_SUCCESS;
}
inline Device& device(void* user) { return *static_cast<Device*>(user); }
inline int32_t create_layouts(void* user, rsf_sharpenvk_layouts* out) {
    auto& d=device(user);
    VkDescriptorSetLayoutBinding bindings[6]{};
    for (uint32_t i=0;i<6;++i) { bindings[i].binding=i; bindings[i].descriptorCount=1; bindings[i].stageFlags=VK_SHADER_STAGE_COMPUTE_BIT; bindings[i].descriptorType=binding_type(i); }
    VkDescriptorSetLayoutCreateInfo info{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO}; info.bindingCount=6; info.pBindings=bindings;
    VkDescriptorSetLayout layout=VK_NULL_HANDLE;
    auto result=d.CreateDescriptorSetLayout(d.device,&info,nullptr,&layout); out->descriptor_set=value(layout);
    if (result!=VK_SUCCESS) return result;
    VkPipelineLayoutCreateInfo pipeline{VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO}; pipeline.setLayoutCount=1; pipeline.pSetLayouts=&layout;
    VkPipelineLayout pipeline_layout=VK_NULL_HANDLE;
    result=d.CreatePipelineLayout(d.device,&pipeline,nullptr,&pipeline_layout); out->pipeline=value(pipeline_layout);
    return result;
}
inline void release_layouts(void* user, rsf_sharpenvk_layouts* layouts) {
    auto& d=device(user);
    if (layouts->pipeline) d.DestroyPipelineLayout(d.device,handle<VkPipelineLayout_T>(layouts->pipeline),nullptr);
    if (layouts->descriptor_set) d.DestroyDescriptorSetLayout(d.device,handle<VkDescriptorSetLayout_T>(layouts->descriptor_set),nullptr);
    *layouts={};
}
inline int32_t create_pipeline(void* user, uint64_t layout, uint32_t mode, uint64_t* out) {
    auto& d=device(user);
    const uint32_t* code[]={rcas_spirv,da_rcas_spirv,da_das_spirv};
    const size_t sizes[]={sizeof(rcas_spirv),sizeof(da_rcas_spirv),sizeof(da_das_spirv)};
    if (mode<1 || mode>3) return VK_ERROR_INITIALIZATION_FAILED;
    VkShaderModule module=VK_NULL_HANDLE; VkShaderModuleCreateInfo shader{VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO}; shader.codeSize=sizes[mode-1]; shader.pCode=code[mode-1];
    auto result=d.CreateShaderModule(d.device,&shader,nullptr,&module);
    if (result!=VK_SUCCESS) return result;
    VkComputePipelineCreateInfo info{VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO}; info.layout=handle<VkPipelineLayout_T>(layout);
    info.stage.sType=VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO; info.stage.stage=VK_SHADER_STAGE_COMPUTE_BIT; info.stage.module=module; info.stage.pName="CSMain";
    VkPipeline pipeline=VK_NULL_HANDLE;
    result=d.CreateComputePipelines(d.device,VK_NULL_HANDLE,1,&info,nullptr,&pipeline); d.DestroyShaderModule(d.device,module,nullptr);
    *out=value(pipeline);
    return result;
}
inline uint32_t memory_type(const Device& d,uint32_t bits,VkMemoryPropertyFlags flags) {
    for (uint32_t i=0;i<d.memory.memoryTypeCount;++i)
        if ((bits&(1u<<i)) && (d.memory.memoryTypes[i].propertyFlags&flags)==flags) return i;
    return UINT32_MAX;
}
inline int32_t create_uniform(void* user, uint64_t layout, rsf_sharpenvk_uniform* slot) {
    auto& d=device(user);
    VkDescriptorPoolSize sizes[]={{VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,1},{VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,4},{VK_DESCRIPTOR_TYPE_STORAGE_IMAGE,1}};
    VkDescriptorPoolCreateInfo p{VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO}; p.maxSets=1; p.poolSizeCount=3; p.pPoolSizes=sizes;
    VkDescriptorPool pool=VK_NULL_HANDLE; auto result=d.CreateDescriptorPool(d.device,&p,nullptr,&pool); slot->pool=value(pool);
    if (result!=VK_SUCCESS) return result;
    auto set_layout=handle<VkDescriptorSetLayout_T>(layout);
    VkDescriptorSetAllocateInfo set{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO}; set.descriptorPool=pool; set.descriptorSetCount=1; set.pSetLayouts=&set_layout;
    VkDescriptorSet descriptors=VK_NULL_HANDLE; result=d.AllocateDescriptorSets(d.device,&set,&descriptors); slot->set=value(descriptors);
    if (result!=VK_SUCCESS) return result;
    VkBufferCreateInfo buffer{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO}; buffer.size=sizeof(Constants); buffer.usage=VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT;
    VkBuffer constants=VK_NULL_HANDLE; result=d.CreateBuffer(d.device,&buffer,nullptr,&constants); slot->buffer=value(constants);
    if (result!=VK_SUCCESS) return result;
    VkMemoryRequirements req{}; d.GetBufferMemoryRequirements(d.device,constants,&req);
    const auto type=memory_type(d,req.memoryTypeBits,VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
    if (type==UINT32_MAX) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    VkMemoryAllocateInfo allocation{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocation.allocationSize=req.size; allocation.memoryTypeIndex=type;
    VkDeviceMemory memory=VK_NULL_HANDLE; result=d.images.allocate(d.device,&allocation,nullptr,&memory); slot->memory=value(memory);
    if (result!=VK_SUCCESS) return result;
    result=d.BindBufferMemory(d.device,constants,memory,0);
    if (result!=VK_SUCCESS) return result;
    return d.MapMemory(d.device,memory,0,VK_WHOLE_SIZE,0,&slot->mapped);
}
inline void release_uniform(void* user, rsf_sharpenvk_uniform* slot) {
    auto& d=device(user);
    if (slot->mapped) d.UnmapMemory(d.device,handle<VkDeviceMemory_T>(slot->memory));
    if (slot->buffer) d.DestroyBuffer(d.device,handle<VkBuffer_T>(slot->buffer),nullptr);
    if (slot->memory) d.images.free_memory(d.device,handle<VkDeviceMemory_T>(slot->memory),nullptr);
    if (slot->pool) d.DestroyDescriptorPool(d.device,handle<VkDescriptorPool_T>(slot->pool),nullptr);
    *slot={};
}
inline int32_t create_view(void* user, uint64_t image, uint32_t format, uint64_t* out) {
    auto& d=device(user);
    VkImageViewCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO}; info.image=handle<VkImage_T>(image); info.viewType=VK_IMAGE_VIEW_TYPE_2D; info.format=static_cast<VkFormat>(format);
    info.subresourceRange={static_cast<VkImageAspectFlags>(format==VK_FORMAT_D32_SFLOAT?VK_IMAGE_ASPECT_DEPTH_BIT:VK_IMAGE_ASPECT_COLOR_BIT),0,1,0,1};
    VkImageView view=VK_NULL_HANDLE; const auto result=d.images.create_view(d.device,&info,nullptr,&view); *out=value(view);
    return result;
}
inline uint32_t options(const rsf_sharpening_options* request, rsf_sharpening_options* out) {
    auto value=request?*request:rsf_sharpening_defaults();
    const auto o=sanitize(options_from_request(value));
    value.mode=static_cast<uint32_t>(o.mode); value.sharpness=o.sharpness; value.mas=o.mas;
    value.motion_sharpness=o.motion_sharpness; value.motion_threshold=o.motion_threshold; value.motion_scale_limit=o.motion_scale_limit;
    value.contrast_enabled=o.contrast_enabled; value.contrast=o.contrast; value.depth_scale=o.depth_scale; value.depth_bias=o.depth_bias;
    value.clamp_output=o.clamp_output; value.debug=o.debug;
    *out=value;
    return active(o)?1u:0u;
}
inline void write_constants(void* mapped, const rsf_sharpening_options* options, const rsf_sharpenvk_facts* f) {
    Frame frame{}; frame.width=f->width; frame.height=f->height;
    frame.motion_width=f->motion_width; frame.motion_height=f->motion_height;
    frame.depth_width=f->depth_width; frame.depth_height=f->depth_height;
    frame.motion_x=f->motion_x; frame.motion_y=f->motion_y;
    frame.depth_inverted=f->depth_inverted; frame.near_plane=f->near_plane; frame.far_plane=f->far_plane;
    frame.exposure_texture=f->exposure_texture; frame.exposure=f->exposure;
    const auto data=constants(options_from_request(*options),frame,static_cast<Mode>(f->effective_mode)); std::memcpy(mapped,&data,sizeof(data));
}
inline void barrier(const Device& d,VkCommandBuffer commands,VkImage image,VkImageLayout before,VkImageLayout after,VkAccessFlags from,VkAccessFlags to) {
    VkImageMemoryBarrier b{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER}; b.image=image; b.oldLayout=before; b.newLayout=after;
    b.srcAccessMask=from; b.dstAccessMask=to; b.srcQueueFamilyIndex=b.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
    b.subresourceRange={VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
    d.CmdPipelineBarrier(commands,VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,0,0,nullptr,0,nullptr,1,&b);
}
inline void record(void* user, void* raw, const rsf_sharpenvk_pass* pass) {
    const auto& d=device(user);
    const auto set=handle<VkDescriptorSet_T>(pass->set);
    VkDescriptorBufferInfo buffer{handle<VkBuffer_T>(pass->buffer),0,sizeof(Constants)}; VkDescriptorImageInfo image[5]{}; VkWriteDescriptorSet writes[6]{};
    for (uint32_t i=0;i<5;++i) {
        image[i].imageView=handle<VkImageView_T>(i==4?pass->scratch_view:pass->sampled[i]);
        image[i].imageLayout=i==4?VK_IMAGE_LAYOUT_GENERAL:VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
    }
    for (uint32_t i=0;i<6;++i) { writes[i].sType=VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET; writes[i].dstSet=set; writes[i].dstBinding=i; writes[i].descriptorCount=1;
        writes[i].descriptorType=binding_type(i);
        if (!i) writes[i].pBufferInfo=&buffer; else writes[i].pImageInfo=&image[i-1]; }
    d.UpdateDescriptorSets(d.device,6,writes,0,nullptr);
    const auto commands=static_cast<VkCommandBuffer>(raw);
    const auto output=handle<VkImage_T>(pass->output), scratch=handle<VkImage_T>(pass->scratch);
    barrier(d,commands,output,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,VK_ACCESS_MEMORY_WRITE_BIT,VK_ACCESS_SHADER_READ_BIT);
    barrier(d,commands,scratch,pass->scratch_initialized?VK_IMAGE_LAYOUT_GENERAL:VK_IMAGE_LAYOUT_UNDEFINED,VK_IMAGE_LAYOUT_GENERAL,0,VK_ACCESS_SHADER_WRITE_BIT);
    d.CmdBindPipeline(commands,VK_PIPELINE_BIND_POINT_COMPUTE,handle<VkPipeline_T>(pass->pipeline));
    d.CmdBindDescriptorSets(commands,VK_PIPELINE_BIND_POINT_COMPUTE,handle<VkPipelineLayout_T>(pass->pipeline_layout),0,1,&set,0,nullptr);
    d.CmdDispatch(commands,pass->groups_x,pass->groups_y,1);
    barrier(d,commands,scratch,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,VK_ACCESS_SHADER_WRITE_BIT,VK_ACCESS_TRANSFER_READ_BIT);
    barrier(d,commands,output,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_ACCESS_SHADER_READ_BIT,VK_ACCESS_TRANSFER_WRITE_BIT);
    VkImageCopy copy{}; copy.srcSubresource=copy.dstSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1}; copy.extent={pass->width,pass->height,1};
    d.CmdCopyImage(commands,scratch,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,output,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&copy);
    barrier(d,commands,output,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL,VK_ACCESS_TRANSFER_WRITE_BIT,VK_ACCESS_MEMORY_READ_BIT|VK_ACCESS_MEMORY_WRITE_BIT);
    barrier(d,commands,scratch,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL,VK_ACCESS_TRANSFER_READ_BIT,0);
}
inline void report_failure(const rsf_srvk_frame* frame, const char* message) noexcept try {
    if (frame->log) frame->log(frame->log_user, message);
    else OutputDebugStringA(message);
} catch (...) {}
inline const rsf_sharpenvk_platform platform{
    &rsf::vkfg::platform, open, [](void* user) { delete static_cast<Device*>(user); },
    [](void* user) -> void* { return &device(user).images; },
    create_layouts, release_layouts, create_pipeline,
    [](void* user, uint64_t pipeline) { auto& d=device(user); d.DestroyPipeline(d.device,handle<VkPipeline_T>(pipeline),nullptr); },
    create_uniform, release_uniform, create_view, options, write_constants, record,
};
}

// Native facade over the Rust owner. The recording owner serializes calls.
// retire() requires completed GPU work AND reset of the named command buffer.
// clear() and destruction require every user drained.
class Vulkan {
    rsf_sharpenvk* owner=nullptr;
    bool failure_logged = false;
public:
    Vulkan()=default;
    Vulkan(const Vulkan&)=delete;
    Vulkan& operator=(const Vulkan&)=delete;
    ~Vulkan() { rsf_core_sharpenvk_destroy(owner); }
    void clear() { if (owner) rsf_core_sharpenvk_clear(owner); }
    void retire(void* commands) { if (owner) rsf_core_sharpenvk_retire(owner,commands); }
    uint64_t dispatches() const { return owner?rsf_core_sharpenvk_dispatches(owner):0; }
    // Logs the first failure through input.log, or the debugger when absent.
    bool run(const rsf_srvk_setup& setup, void* commands, const rsf_srvk_frame& input, VkImage output) {
        if (!owner) owner = rsf_core_sharpenvk_create(&vulkan_leaf::platform);
        if (owner && rsf_core_sharpenvk_record(owner, &setup, commands, &input, vulkan_leaf::value(output))) return true;
        if (!failure_logged) {
            failure_logged = true;
            vulkan_leaf::report_failure(&input, "Vulkan sharpening unavailable; retaining vendor output");
        }
        return false;
    }
};
}
