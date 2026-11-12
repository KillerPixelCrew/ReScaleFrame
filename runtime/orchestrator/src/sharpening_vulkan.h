// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <vulkan/vulkan.h>
#include "../../graphics/src/sharpening/options.h"
#include "../../graphics/src/sharpening/rcas_spirv.h"
#include "../../graphics/src/sharpening/da_rcas_spirv.h"
#include "../../graphics/src/sharpening/da_das_spirv.h"
#include <windows.h>
#include <memory>
#include <vector>
#include <cstring>
namespace rsf::sharpening {
// Recording owner serializes calls. retire() requires completed GPU work AND reset
// of the named command buffer. Context destruction requires every user drained.
class Vulkan {
    struct Slot {
        VkCommandBuffer commands=VK_NULL_HANDLE;
        VkDescriptorPool pool=VK_NULL_HANDLE; VkDescriptorSet set=VK_NULL_HANDLE;
        VkBuffer constants=VK_NULL_HANDLE; VkDeviceMemory constant_memory=VK_NULL_HANDLE; void* mapped=nullptr;
        VkImage scratch=VK_NULL_HANDLE; VkDeviceMemory scratch_memory=VK_NULL_HANDLE;
        VkImageView scratch_view=VK_NULL_HANDLE, views[4]{};
        uint32_t width=0,height=0; bool initialized=false;
    };
    VkDevice device=VK_NULL_HANDLE;
    VkPhysicalDeviceMemoryProperties memory{};
    VkDescriptorSetLayout layout=VK_NULL_HANDLE; VkPipelineLayout pipeline_layout=VK_NULL_HANDLE;
    VkPipeline pipelines[3]{};
    std::vector<std::unique_ptr<Slot>> slots;
    uint64_t dispatch_count=0;
    bool failure_logged=false;
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
    PFN_vkAllocateMemory AllocateMemory=nullptr;
    PFN_vkFreeMemory FreeMemory=nullptr;
    PFN_vkBindBufferMemory BindBufferMemory=nullptr;
    PFN_vkMapMemory MapMemory=nullptr;
    PFN_vkUnmapMemory UnmapMemory=nullptr;
    PFN_vkCreateImage CreateImage=nullptr;
    PFN_vkDestroyImage DestroyImage=nullptr;
    PFN_vkGetImageMemoryRequirements GetImageMemoryRequirements=nullptr;
    PFN_vkBindImageMemory BindImageMemory=nullptr;
    PFN_vkCreateImageView CreateImageView=nullptr;
    PFN_vkDestroyImageView DestroyImageView=nullptr;
    PFN_vkCmdPipelineBarrier CmdPipelineBarrier=nullptr;
    PFN_vkCmdBindPipeline CmdBindPipeline=nullptr;
    PFN_vkCmdBindDescriptorSets CmdBindDescriptorSets=nullptr;
    PFN_vkCmdDispatch CmdDispatch=nullptr;
    PFN_vkCmdCopyImage CmdCopyImage=nullptr;

    uint32_t memory_type(uint32_t bits,VkMemoryPropertyFlags flags) {
        for (uint32_t i=0;i<memory.memoryTypeCount;++i)
            if ((bits&(1u<<i)) && (memory.memoryTypes[i].propertyFlags&flags)==flags) return i;
        return UINT32_MAX;
    }
    bool allocate(const VkMemoryRequirements& req,VkMemoryPropertyFlags flags,VkDeviceMemory& out) {
        const auto type=memory_type(req.memoryTypeBits,flags); if (type==UINT32_MAX) return false;
        VkMemoryAllocateInfo info{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; info.allocationSize=req.size; info.memoryTypeIndex=type;
        return AllocateMemory(device,&info,nullptr,&out)==VK_SUCCESS;
    }
    void release_views(Slot& slot) {
        for (auto& view:slot.views) if (view) { DestroyImageView(device,view,nullptr); view=VK_NULL_HANDLE; }
    }
    void release(Slot& slot) {
        release_views(slot);
        if (slot.scratch_view) DestroyImageView(device,slot.scratch_view,nullptr);
        if (slot.scratch) DestroyImage(device,slot.scratch,nullptr);
        if (slot.scratch_memory) FreeMemory(device,slot.scratch_memory,nullptr);
        if (slot.mapped) UnmapMemory(device,slot.constant_memory);
        if (slot.constants) DestroyBuffer(device,slot.constants,nullptr);
        if (slot.constant_memory) FreeMemory(device,slot.constant_memory,nullptr);
        if (slot.pool) DestroyDescriptorPool(device,slot.pool,nullptr);
    }
    bool initialize(const rsf_srvk_setup& setup) {
        if (device) return device==setup.device && pipelines[0] && pipelines[1] && pipelines[2];
        device=static_cast<VkDevice>(setup.device);
        CreateShaderModule=reinterpret_cast<PFN_vkCreateShaderModule>(setup.device_proc(setup.device,"vkCreateShaderModule")); if (!CreateShaderModule) return false;
        DestroyShaderModule=reinterpret_cast<PFN_vkDestroyShaderModule>(setup.device_proc(setup.device,"vkDestroyShaderModule")); if (!DestroyShaderModule) return false;
        CreateDescriptorSetLayout=reinterpret_cast<PFN_vkCreateDescriptorSetLayout>(setup.device_proc(setup.device,"vkCreateDescriptorSetLayout")); if (!CreateDescriptorSetLayout) return false;
        DestroyDescriptorSetLayout=reinterpret_cast<PFN_vkDestroyDescriptorSetLayout>(setup.device_proc(setup.device,"vkDestroyDescriptorSetLayout")); if (!DestroyDescriptorSetLayout) return false;
        CreatePipelineLayout=reinterpret_cast<PFN_vkCreatePipelineLayout>(setup.device_proc(setup.device,"vkCreatePipelineLayout")); if (!CreatePipelineLayout) return false;
        DestroyPipelineLayout=reinterpret_cast<PFN_vkDestroyPipelineLayout>(setup.device_proc(setup.device,"vkDestroyPipelineLayout")); if (!DestroyPipelineLayout) return false;
        CreateComputePipelines=reinterpret_cast<PFN_vkCreateComputePipelines>(setup.device_proc(setup.device,"vkCreateComputePipelines")); if (!CreateComputePipelines) return false;
        DestroyPipeline=reinterpret_cast<PFN_vkDestroyPipeline>(setup.device_proc(setup.device,"vkDestroyPipeline")); if (!DestroyPipeline) return false;
        CreateDescriptorPool=reinterpret_cast<PFN_vkCreateDescriptorPool>(setup.device_proc(setup.device,"vkCreateDescriptorPool")); if (!CreateDescriptorPool) return false;
        DestroyDescriptorPool=reinterpret_cast<PFN_vkDestroyDescriptorPool>(setup.device_proc(setup.device,"vkDestroyDescriptorPool")); if (!DestroyDescriptorPool) return false;
        AllocateDescriptorSets=reinterpret_cast<PFN_vkAllocateDescriptorSets>(setup.device_proc(setup.device,"vkAllocateDescriptorSets")); if (!AllocateDescriptorSets) return false;
        UpdateDescriptorSets=reinterpret_cast<PFN_vkUpdateDescriptorSets>(setup.device_proc(setup.device,"vkUpdateDescriptorSets")); if (!UpdateDescriptorSets) return false;
        CreateBuffer=reinterpret_cast<PFN_vkCreateBuffer>(setup.device_proc(setup.device,"vkCreateBuffer")); if (!CreateBuffer) return false;
        DestroyBuffer=reinterpret_cast<PFN_vkDestroyBuffer>(setup.device_proc(setup.device,"vkDestroyBuffer")); if (!DestroyBuffer) return false;
        GetBufferMemoryRequirements=reinterpret_cast<PFN_vkGetBufferMemoryRequirements>(setup.device_proc(setup.device,"vkGetBufferMemoryRequirements")); if (!GetBufferMemoryRequirements) return false;
        AllocateMemory=reinterpret_cast<PFN_vkAllocateMemory>(setup.device_proc(setup.device,"vkAllocateMemory")); if (!AllocateMemory) return false;
        FreeMemory=reinterpret_cast<PFN_vkFreeMemory>(setup.device_proc(setup.device,"vkFreeMemory")); if (!FreeMemory) return false;
        BindBufferMemory=reinterpret_cast<PFN_vkBindBufferMemory>(setup.device_proc(setup.device,"vkBindBufferMemory")); if (!BindBufferMemory) return false;
        MapMemory=reinterpret_cast<PFN_vkMapMemory>(setup.device_proc(setup.device,"vkMapMemory")); if (!MapMemory) return false;
        UnmapMemory=reinterpret_cast<PFN_vkUnmapMemory>(setup.device_proc(setup.device,"vkUnmapMemory")); if (!UnmapMemory) return false;
        CreateImage=reinterpret_cast<PFN_vkCreateImage>(setup.device_proc(setup.device,"vkCreateImage")); if (!CreateImage) return false;
        DestroyImage=reinterpret_cast<PFN_vkDestroyImage>(setup.device_proc(setup.device,"vkDestroyImage")); if (!DestroyImage) return false;
        GetImageMemoryRequirements=reinterpret_cast<PFN_vkGetImageMemoryRequirements>(setup.device_proc(setup.device,"vkGetImageMemoryRequirements")); if (!GetImageMemoryRequirements) return false;
        BindImageMemory=reinterpret_cast<PFN_vkBindImageMemory>(setup.device_proc(setup.device,"vkBindImageMemory")); if (!BindImageMemory) return false;
        CreateImageView=reinterpret_cast<PFN_vkCreateImageView>(setup.device_proc(setup.device,"vkCreateImageView")); if (!CreateImageView) return false;
        DestroyImageView=reinterpret_cast<PFN_vkDestroyImageView>(setup.device_proc(setup.device,"vkDestroyImageView")); if (!DestroyImageView) return false;
        CmdPipelineBarrier=reinterpret_cast<PFN_vkCmdPipelineBarrier>(setup.device_proc(setup.device,"vkCmdPipelineBarrier")); if (!CmdPipelineBarrier) return false;
        CmdBindPipeline=reinterpret_cast<PFN_vkCmdBindPipeline>(setup.device_proc(setup.device,"vkCmdBindPipeline")); if (!CmdBindPipeline) return false;
        CmdBindDescriptorSets=reinterpret_cast<PFN_vkCmdBindDescriptorSets>(setup.device_proc(setup.device,"vkCmdBindDescriptorSets")); if (!CmdBindDescriptorSets) return false;
        CmdDispatch=reinterpret_cast<PFN_vkCmdDispatch>(setup.device_proc(setup.device,"vkCmdDispatch")); if (!CmdDispatch) return false;
        CmdCopyImage=reinterpret_cast<PFN_vkCmdCopyImage>(setup.device_proc(setup.device,"vkCmdCopyImage")); if (!CmdCopyImage) return false;

        auto properties=reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup.instance_proc(setup.instance,"vkGetPhysicalDeviceMemoryProperties"));
        if (!properties) return false;
        properties(static_cast<VkPhysicalDevice>(setup.physical_device),&memory);
        VkDescriptorSetLayoutBinding bindings[6]{};
        for (uint32_t i=0;i<6;++i) { bindings[i].binding=i; bindings[i].descriptorCount=1; bindings[i].stageFlags=VK_SHADER_STAGE_COMPUTE_BIT;
            bindings[i].descriptorType=i==0?VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER:i==5?VK_DESCRIPTOR_TYPE_STORAGE_IMAGE:VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE; }
        VkDescriptorSetLayoutCreateInfo d{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO}; d.bindingCount=6; d.pBindings=bindings;
        if (CreateDescriptorSetLayout(device,&d,nullptr,&layout)!=VK_SUCCESS) return false;
        VkPipelineLayoutCreateInfo p{VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO}; p.setLayoutCount=1; p.pSetLayouts=&layout;
        if (CreatePipelineLayout(device,&p,nullptr,&pipeline_layout)!=VK_SUCCESS) return false;
        const uint32_t* code[]={rcas_spirv,da_rcas_spirv,da_das_spirv};
        const size_t sizes[]={sizeof(rcas_spirv),sizeof(da_rcas_spirv),sizeof(da_das_spirv)};
        for (uint32_t i=0;i<3;++i) {
            VkShaderModule module=VK_NULL_HANDLE; VkShaderModuleCreateInfo shader{VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO}; shader.codeSize=sizes[i]; shader.pCode=code[i];
            if (CreateShaderModule(device,&shader,nullptr,&module)!=VK_SUCCESS) return false;
            VkComputePipelineCreateInfo info{VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO}; info.layout=pipeline_layout;
            info.stage.sType=VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO; info.stage.stage=VK_SHADER_STAGE_COMPUTE_BIT; info.stage.module=module; info.stage.pName="CSMain";
            auto result=CreateComputePipelines(device,VK_NULL_HANDLE,1,&info,nullptr,&pipelines[i]); DestroyShaderModule(device,module,nullptr);
            if (result!=VK_SUCCESS) return false;
        }
        return true;
    }
    bool view(VkImage image,VkFormat format,VkImageView& out) {
        VkImageViewCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO}; info.image=image; info.viewType=VK_IMAGE_VIEW_TYPE_2D; info.format=format;
        info.subresourceRange={static_cast<VkImageAspectFlags>(format==VK_FORMAT_D32_SFLOAT?VK_IMAGE_ASPECT_DEPTH_BIT:VK_IMAGE_ASPECT_COLOR_BIT),0,1,0,1};
        return CreateImageView(device,&info,nullptr,&out)==VK_SUCCESS;
    }
    bool prepare(Slot& slot,uint32_t width,uint32_t height) {
        if (!slot.pool) {
            VkDescriptorPoolSize sizes[]={{VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,1},{VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,4},{VK_DESCRIPTOR_TYPE_STORAGE_IMAGE,1}};
            VkDescriptorPoolCreateInfo p{VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO}; p.maxSets=1; p.poolSizeCount=3; p.pPoolSizes=sizes;
            if (CreateDescriptorPool(device,&p,nullptr,&slot.pool)!=VK_SUCCESS) return false;
            VkDescriptorSetAllocateInfo set{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO}; set.descriptorPool=slot.pool; set.descriptorSetCount=1; set.pSetLayouts=&layout;
            if (AllocateDescriptorSets(device,&set,&slot.set)!=VK_SUCCESS) return false;
            VkBufferCreateInfo buffer{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO}; buffer.size=sizeof(Constants); buffer.usage=VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT;
            if (CreateBuffer(device,&buffer,nullptr,&slot.constants)!=VK_SUCCESS) return false;
            VkMemoryRequirements req{}; GetBufferMemoryRequirements(device,slot.constants,&req);
            if (!allocate(req,VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,slot.constant_memory) ||
                BindBufferMemory(device,slot.constants,slot.constant_memory,0)!=VK_SUCCESS ||
                MapMemory(device,slot.constant_memory,0,VK_WHOLE_SIZE,0,&slot.mapped)!=VK_SUCCESS) return false;
        }
        if (slot.width!=width || slot.height!=height) {
            if (slot.scratch_view) { DestroyImageView(device,slot.scratch_view,nullptr); slot.scratch_view=VK_NULL_HANDLE; }
            if (slot.scratch) { DestroyImage(device,slot.scratch,nullptr); slot.scratch=VK_NULL_HANDLE; }
            if (slot.scratch_memory) { FreeMemory(device,slot.scratch_memory,nullptr); slot.scratch_memory=VK_NULL_HANDLE; }
            VkImageCreateInfo image{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}; image.imageType=VK_IMAGE_TYPE_2D; image.format=VK_FORMAT_R16G16B16A16_SFLOAT;
            image.extent={width,height,1}; image.mipLevels=image.arrayLayers=1; image.samples=VK_SAMPLE_COUNT_1_BIT; image.tiling=VK_IMAGE_TILING_OPTIMAL;
            image.usage=VK_IMAGE_USAGE_STORAGE_BIT|VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
            if (CreateImage(device,&image,nullptr,&slot.scratch)!=VK_SUCCESS) return false;
            VkMemoryRequirements req{}; GetImageMemoryRequirements(device,slot.scratch,&req);
            if (!allocate(req,VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT,slot.scratch_memory) ||
                BindImageMemory(device,slot.scratch,slot.scratch_memory,0)!=VK_SUCCESS || !view(slot.scratch,image.format,slot.scratch_view)) return false;
            slot.width=width; slot.height=height; slot.initialized=false;
        }
        return slot.mapped && slot.set && slot.scratch_view;
    }
    void barrier(VkCommandBuffer commands,VkImage image,VkImageLayout before,VkImageLayout after,VkAccessFlags from,VkAccessFlags to) {
        VkImageMemoryBarrier b{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER}; b.image=image; b.oldLayout=before; b.newLayout=after;
        b.srcAccessMask=from; b.dstAccessMask=to; b.srcQueueFamilyIndex=b.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
        b.subresourceRange={VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
        CmdPipelineBarrier(commands,VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,0,0,nullptr,0,nullptr,1,&b);
    }
public:
    ~Vulkan() { clear(); }
    void clear() {
        if (!device) return;
        for (auto& slot:slots) release(*slot);
        for (auto pipeline:pipelines) if (pipeline) DestroyPipeline(device,pipeline,nullptr);
        if (pipeline_layout) DestroyPipelineLayout(device,pipeline_layout,nullptr);
        if (layout) DestroyDescriptorSetLayout(device,layout,nullptr);
        slots.clear(); for (auto& pipeline:pipelines) pipeline=VK_NULL_HANDLE;
        layout=VK_NULL_HANDLE; pipeline_layout=VK_NULL_HANDLE; device=VK_NULL_HANDLE;
    }
    void retire(void* commands) {
        for (auto& slot:slots) if (slot->commands==commands) { release_views(*slot); slot->commands=VK_NULL_HANDLE; }
    }
    uint64_t dispatches() const { return dispatch_count; }
    bool run(const rsf_srvk_setup& setup,void* raw,const rsf_srvk_frame& input,VkImage output) {
        const bool result=execute(setup,raw,input,output);
        if (!result && !failure_logged) { failure_logged=true; const char* message="Vulkan sharpening unavailable; retaining vendor output"; if (input.log) input.log(input.log_user,message); else OutputDebugStringA(message); }
        return result;
    }
    bool execute(const rsf_srvk_setup& setup,void* raw,const rsf_srvk_frame& input,VkImage output) try {
        const auto request=input.sharpening.struct_size?input.sharpening:rsf_sharpening_defaults();
        Options options;
        options.mode=static_cast<Mode>(request.mode);
        options.sharpness=request.sharpness;
        options.mas=request.mas;
        options.motion_sharpness=request.motion_sharpness;
        options.motion_threshold=request.motion_threshold;
        options.motion_scale_limit=request.motion_scale_limit;
        options.contrast_enabled=request.contrast_enabled;
        options.contrast=request.contrast;
        options.depth_scale=request.depth_scale;
        options.depth_bias=request.depth_bias;
        options.clamp_output=request.clamp_output;
        options.debug=request.debug;

        options=sanitize(options);
        if (!active(options)) return true;
        if (!raw || !output || !input.record) return false;
        for (const auto* image:{&input.motion,&input.depth,&input.exposure})
            if (image->image && (image->device!=setup.device || image->layout!=VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL || !image->width || !image->height)) return false;
        if (input.exposure.image && (input.exposure.width!=1 || input.exposure.height!=1 ||
            (input.exposure.format!=VK_FORMAT_R32_SFLOAT && input.exposure.format!=VK_FORMAT_R16_SFLOAT))) return false;
        if (!initialize(setup)) return false;
        Slot* slot=nullptr;
        for (auto& entry:slots) if (!entry->commands) { slot=entry.get(); break; }
        // A bounded recording pool. An absent retirement callback degrades safely.
        if (!slot && slots.size()<64) { slots.push_back(std::make_unique<Slot>()); slot=slots.back().get(); }
        if (!slot) return false;
        if (!prepare(*slot,input.record->output_width,input.record->output_height)) { release(*slot); *slot=Slot{}; return false; }
        slot->commands=static_cast<VkCommandBuffer>(raw);
        release_views(*slot);
        if (!view(output,VK_FORMAT_R16G16B16A16_SFLOAT,slot->views[0])) return false;
        const rsf_srvk_image* images[]={&input.motion,&input.depth,&input.exposure};
        for (uint32_t i=0;i<3;++i) if (images[i]->image && !view(reinterpret_cast<VkImage>(images[i]->image),static_cast<VkFormat>(images[i]->format),slot->views[i+1])) return false;
        Frame frame{}; frame.width=slot->width; frame.height=slot->height;
        frame.motion_width=input.motion.width; frame.motion_height=input.motion.height;
        frame.depth_width=input.depth.width; frame.depth_height=input.depth.height;
        frame.motion_x=input.motion_scale_x; frame.motion_y=input.motion_scale_y;
        frame.depth_inverted=input.record->camera.depth_inverted; frame.near_plane=input.record->camera.near_plane; frame.far_plane=input.record->camera.far_plane;
        frame.exposure_texture=input.exposure.image!=0;
        frame.exposure=1.0f/std::max(input.pre_exposure,1e-6f);
        auto effective=options.mode; if (!input.depth.image && (effective==Mode::depth_rcas || effective==Mode::depth_das)) effective=Mode::rcas;
        const auto data=constants(options,frame,effective); std::memcpy(slot->mapped,&data,sizeof(data));
        VkDescriptorBufferInfo buffer{slot->constants,0,sizeof(data)}; VkDescriptorImageInfo image[5]{}; VkWriteDescriptorSet writes[6]{};
        for (uint32_t i=0;i<5;++i) {
            image[i].imageView=i==4?slot->scratch_view:(slot->views[i]?slot->views[i]:slot->views[0]);
            image[i].imageLayout=i==4?VK_IMAGE_LAYOUT_GENERAL:VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
        }
        for (uint32_t i=0;i<6;++i) { writes[i].sType=VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET; writes[i].dstSet=slot->set; writes[i].dstBinding=i; writes[i].descriptorCount=1;
            writes[i].descriptorType=i==0?VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER:i==5?VK_DESCRIPTOR_TYPE_STORAGE_IMAGE:VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE;
            if (!i) writes[i].pBufferInfo=&buffer; else writes[i].pImageInfo=&image[i-1]; }
        UpdateDescriptorSets(device,6,writes,0,nullptr);
        auto commands=static_cast<VkCommandBuffer>(raw);
        barrier(commands,output,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,VK_ACCESS_MEMORY_WRITE_BIT,VK_ACCESS_SHADER_READ_BIT);
        barrier(commands,slot->scratch,slot->initialized?VK_IMAGE_LAYOUT_GENERAL:VK_IMAGE_LAYOUT_UNDEFINED,VK_IMAGE_LAYOUT_GENERAL,0,VK_ACCESS_SHADER_WRITE_BIT);
        CmdBindPipeline(commands,VK_PIPELINE_BIND_POINT_COMPUTE,pipelines[static_cast<unsigned>(effective)-1]);
        CmdBindDescriptorSets(commands,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline_layout,0,1,&slot->set,0,nullptr);
        CmdDispatch(commands,(slot->width+15)/16,(slot->height+15)/16,1); ++dispatch_count;
        barrier(commands,slot->scratch,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,VK_ACCESS_SHADER_WRITE_BIT,VK_ACCESS_TRANSFER_READ_BIT);
        barrier(commands,output,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_ACCESS_SHADER_READ_BIT,VK_ACCESS_TRANSFER_WRITE_BIT);
        VkImageCopy copy{}; copy.srcSubresource=copy.dstSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1}; copy.extent={slot->width,slot->height,1};
        CmdCopyImage(commands,slot->scratch,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,output,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&copy);
        barrier(commands,output,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL,VK_ACCESS_TRANSFER_WRITE_BIT,VK_ACCESS_MEMORY_READ_BIT|VK_ACCESS_MEMORY_WRITE_BIT);
        barrier(commands,slot->scratch,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL,VK_ACCESS_TRANSFER_READ_BIT,0);
        slot->commands=commands; slot->initialized=true;
        return true;
    } catch (...) { return false; }
};
}
