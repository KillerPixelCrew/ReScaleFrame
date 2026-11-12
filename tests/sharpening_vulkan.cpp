// SPDX-License-Identifier: GPL-3.0-only
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include <DirectXPackedVector.h>
#include <rescaleframe/native_latency_vulkan.h>
#include "../runtime/orchestrator/src/sharpening_vulkan.h"
#include <stdexcept>
#include <vector>
#include <array>
#include <cstdio>
#include <cmath>
#include <cstring>
namespace {
void require(bool value,const char* message) { if(!value)throw std::runtime_error(message); }
struct Image { VkImage image = VK_NULL_HANDLE; VkDeviceMemory memory = VK_NULL_HANDLE; uint32_t width, height; VkFormat format; uint32_t copy_source = 0; };
struct Gpu {
    HMODULE module = nullptr;
    VkInstance instance = VK_NULL_HANDLE;
    VkPhysicalDevice physical = VK_NULL_HANDLE;
    VkDevice device = VK_NULL_HANDLE;
    VkQueue queue = VK_NULL_HANDLE;
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandBuffer commands = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE;
    VkBuffer readback = VK_NULL_HANDLE;
    VkDeviceMemory readback_memory = VK_NULL_HANDLE;
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkGetInstanceProcAddr gip = nullptr;
    PFN_vkGetDeviceProcAddr gdp = nullptr;
    PFN_vkDestroyInstance destroy_instance = nullptr;
    PFN_vkDestroyDevice destroy_device = nullptr;
    PFN_vkDeviceWaitIdle idle = nullptr;
    PFN_vkCreateImage create_image = nullptr;
    PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements image_requirements = nullptr;
    PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr;
    PFN_vkBindImageMemory bind_image = nullptr;
    PFN_vkCreateBuffer create_buffer = nullptr;
    PFN_vkDestroyBuffer destroy_buffer = nullptr;
    PFN_vkGetBufferMemoryRequirements buffer_requirements = nullptr;
    PFN_vkBindBufferMemory bind_buffer = nullptr;
    PFN_vkMapMemory map = nullptr;
    PFN_vkUnmapMemory unmap = nullptr;
    PFN_vkCreateCommandPool create_pool = nullptr;
    PFN_vkDestroyCommandPool destroy_pool = nullptr;
    PFN_vkResetCommandPool reset_pool = nullptr;
    PFN_vkAllocateCommandBuffers allocate_commands = nullptr;
    PFN_vkBeginCommandBuffer begin = nullptr;
    PFN_vkEndCommandBuffer end = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdClearColorImage clear = nullptr;
    PFN_vkCmdCopyImageToBuffer copy = nullptr;
    PFN_vkQueueSubmit submit = nullptr;
    PFN_vkCreateFence create_fence = nullptr;
    PFN_vkDestroyFence destroy_fence = nullptr;
    PFN_vkWaitForFences wait = nullptr;
    PFN_vkResetFences reset_fence = nullptr;
    std::vector<Image> images;
    ~Gpu() {
        if (device && idle) (void)idle(device);
        for (const auto& image : images) {
            if (image.image && destroy_image) destroy_image(device, image.image, nullptr);
            if (image.memory && free_memory) free_memory(device, image.memory, nullptr);
        }
        if (readback) destroy_buffer(device, readback, nullptr);
        if (readback_memory) free_memory(device, readback_memory, nullptr);
        if (fence) destroy_fence(device, fence, nullptr);
        if (pool) destroy_pool(device, pool, nullptr);
        if (device && destroy_device) destroy_device(device, nullptr);
        if (instance && destroy_instance) destroy_instance(instance, nullptr);
        if (module) FreeLibrary(module);
    }
    template<class T> T instance_function(const char* name) {
        auto value = reinterpret_cast<T>(gip(instance, name)); require(value != nullptr, name); return value;
    }
    template<class T> T device_function(const char* name) {
        auto value = reinterpret_cast<T>(gdp(device, name)); require(value != nullptr, name); return value;
    }
    void initialize(bool streamline = false) {
        module = LoadLibraryExW(L"vulkan-1.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32); require(module != nullptr, "Vulkan loader");
        gip = reinterpret_cast<PFN_vkGetInstanceProcAddr>(GetProcAddress(module, "vkGetInstanceProcAddr")); require(gip != nullptr, "Vulkan instance dispatch");
        if (streamline) gip = reinterpret_cast<PFN_vkGetInstanceProcAddr>(rsf_latencyvk_instance_proc);
        VkApplicationInfo app{}; app.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO; app.pApplicationName = "ReScaleFrame Vulkan SR fixture"; app.apiVersion = VK_API_VERSION_1_2;
        VkInstanceCreateInfo instance_info{}; instance_info.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO; instance_info.pApplicationInfo = &app;
        const char* instance_extension = VK_KHR_SURFACE_EXTENSION_NAME;
        if (streamline) { instance_info.enabledExtensionCount = 1; instance_info.ppEnabledExtensionNames = &instance_extension; }
        auto create_instance = reinterpret_cast<PFN_vkCreateInstance>(gip(nullptr, "vkCreateInstance")); require(create_instance != nullptr, "create instance entry");
        require(create_instance(&instance_info, nullptr, &instance) == VK_SUCCESS, "create Vulkan instance");
        destroy_instance = instance_function<PFN_vkDestroyInstance>("vkDestroyInstance");
        auto enumerate = instance_function<PFN_vkEnumeratePhysicalDevices>("vkEnumeratePhysicalDevices");
        uint32_t count = 0; require(enumerate(instance, &count, nullptr) == VK_SUCCESS && count, "enumerate GPUs");
        std::vector<VkPhysicalDevice> devices(count); require(enumerate(instance, &count, devices.data()) == VK_SUCCESS, "read GPUs");
        auto properties = instance_function<PFN_vkGetPhysicalDeviceProperties>("vkGetPhysicalDeviceProperties");
        physical = devices[0];
        for (auto candidate : devices) { VkPhysicalDeviceProperties p{}; properties(candidate, &p); if (p.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU) { physical = candidate; break; } }
        VkPhysicalDeviceProperties p{}; properties(physical, &p); std::printf("Vulkan SR fixture GPU: %s\n", p.deviceName);
        auto families = instance_function<PFN_vkGetPhysicalDeviceQueueFamilyProperties>("vkGetPhysicalDeviceQueueFamilyProperties");
        families(physical, &count, nullptr); std::vector<VkQueueFamilyProperties> queues(count); families(physical, &count, queues.data());
        uint32_t family = UINT32_MAX;
        for (uint32_t i = 0; i < count; ++i) if ((queues[i].queueFlags & (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) == (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) { family = i; break; }
        require(family != UINT32_MAX, "graphics/compute family");
        VkPhysicalDeviceVulkan12Features features12{}; features12.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
        VkPhysicalDeviceVulkan11Features features11{}; features11.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_1_FEATURES; features11.pNext = &features12;
        VkPhysicalDeviceFeatures2 features{}; features.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2; features.pNext = &features11;
        instance_function<PFN_vkGetPhysicalDeviceFeatures2>("vkGetPhysicalDeviceFeatures2")(physical, &features);
        float priority = 1; VkDeviceQueueCreateInfo queue_info{}; queue_info.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
        queue_info.queueFamilyIndex = family; queue_info.queueCount = 1; queue_info.pQueuePriorities = &priority;
        VkDeviceCreateInfo device_info{}; device_info.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO; device_info.pNext = &features;
        const char* device_extension = VK_KHR_SWAPCHAIN_EXTENSION_NAME;
        if (streamline) { device_info.enabledExtensionCount = 1; device_info.ppEnabledExtensionNames = &device_extension; }
        device_info.queueCreateInfoCount = 1; device_info.pQueueCreateInfos = &queue_info;
        require(instance_function<PFN_vkCreateDevice>("vkCreateDevice")(physical, &device_info, nullptr, &device) == VK_SUCCESS, "create Vulkan device");
        gdp = instance_function<PFN_vkGetDeviceProcAddr>("vkGetDeviceProcAddr");
#define RSF_LOAD(member, function) member = device_function<PFN_##function>(#function)
        RSF_LOAD(destroy_device, vkDestroyDevice); RSF_LOAD(idle, vkDeviceWaitIdle);
        RSF_LOAD(create_image, vkCreateImage); RSF_LOAD(destroy_image, vkDestroyImage); RSF_LOAD(image_requirements, vkGetImageMemoryRequirements);
        RSF_LOAD(allocate, vkAllocateMemory); RSF_LOAD(free_memory, vkFreeMemory); RSF_LOAD(bind_image, vkBindImageMemory);
        RSF_LOAD(create_buffer, vkCreateBuffer); RSF_LOAD(destroy_buffer, vkDestroyBuffer); RSF_LOAD(buffer_requirements, vkGetBufferMemoryRequirements);
        RSF_LOAD(bind_buffer, vkBindBufferMemory); RSF_LOAD(map, vkMapMemory); RSF_LOAD(unmap, vkUnmapMemory);
        RSF_LOAD(create_pool, vkCreateCommandPool); RSF_LOAD(destroy_pool, vkDestroyCommandPool); RSF_LOAD(reset_pool, vkResetCommandPool);
        RSF_LOAD(allocate_commands, vkAllocateCommandBuffers); RSF_LOAD(begin, vkBeginCommandBuffer); RSF_LOAD(end, vkEndCommandBuffer);
        RSF_LOAD(barrier, vkCmdPipelineBarrier); RSF_LOAD(clear, vkCmdClearColorImage); RSF_LOAD(copy, vkCmdCopyImageToBuffer); RSF_LOAD(submit, vkQueueSubmit);
        RSF_LOAD(create_fence, vkCreateFence); RSF_LOAD(destroy_fence, vkDestroyFence); RSF_LOAD(wait, vkWaitForFences); RSF_LOAD(reset_fence, vkResetFences);
#undef RSF_LOAD
        device_function<PFN_vkGetDeviceQueue>("vkGetDeviceQueue")(device, family, 0, &queue);
        instance_function<PFN_vkGetPhysicalDeviceMemoryProperties>("vkGetPhysicalDeviceMemoryProperties")(physical, &memory);
        VkCommandPoolCreateInfo pool_info{}; pool_info.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO; pool_info.queueFamilyIndex = family;
        require(create_pool(device, &pool_info, nullptr, &pool) == VK_SUCCESS, "create command pool");
        VkCommandBufferAllocateInfo cmd_info{}; cmd_info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO; cmd_info.commandPool = pool; cmd_info.commandBufferCount = 1;
        require(allocate_commands(device, &cmd_info, &commands) == VK_SUCCESS, "allocate commands");
        VkFenceCreateInfo fence_info{}; fence_info.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
        require(create_fence(device, &fence_info, nullptr, &fence) == VK_SUCCESS, "create fence");
        VkBufferCreateInfo buffer{}; buffer.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO; buffer.size = 320 * 180 * 8; buffer.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
        require(create_buffer(device, &buffer, nullptr, &readback) == VK_SUCCESS, "create readback");
        VkMemoryRequirements r{}; buffer_requirements(device, readback, &r);
        VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO; allocation.allocationSize = r.size;
        allocation.memoryTypeIndex = type(r.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
        require(allocate(device, &allocation, nullptr, &readback_memory) == VK_SUCCESS && bind_buffer(device, readback, readback_memory, 0) == VK_SUCCESS, "allocate readback");
    }
    uint32_t type(uint32_t bits, VkMemoryPropertyFlags flags) const {
        for (uint32_t i = 0; i < memory.memoryTypeCount; ++i) if ((bits & (1u << i)) && (memory.memoryTypes[i].propertyFlags & flags) == flags) return i;
        throw std::runtime_error("Vulkan memory type");
    }
    Image image(uint32_t width, uint32_t height, VkFormat format, VkImageUsageFlags usage) {
        Image value{}; value.width = width; value.height = height; value.format = format;
        value.copy_source = (usage & VK_IMAGE_USAGE_TRANSFER_SRC_BIT) != 0;
        VkImageCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO; info.imageType = VK_IMAGE_TYPE_2D;
        info.extent = {width, height, 1}; info.mipLevels = info.arrayLayers = 1; info.format = format; info.samples = VK_SAMPLE_COUNT_1_BIT; info.usage = usage;
        require(create_image(device, &info, nullptr, &value.image) == VK_SUCCESS, "create image");
        images.push_back(value); auto& owned = images.back();
        VkMemoryRequirements r{}; image_requirements(device, value.image, &r);
        VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO; allocation.allocationSize = r.size;
        allocation.memoryTypeIndex = type(r.memoryTypeBits, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
        require(allocate(device, &allocation, nullptr, &owned.memory) == VK_SUCCESS && bind_image(device, owned.image, owned.memory, 0) == VK_SUCCESS, "allocate image");
        return owned;
    }
    void transition(const Image& image, VkImageLayout from, VkImageLayout to) {
        VkImageMemoryBarrier b{}; b.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER; b.oldLayout = from; b.newLayout = to;
        b.srcAccessMask = from == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
        b.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
        b.srcQueueFamilyIndex = b.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED; b.image = image.image;
        b.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &b);
    }
    void start() {
        require(reset_pool(device, pool, 0) == VK_SUCCESS, "reset command pool");
        VkCommandBufferBeginInfo info{}; info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO; info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        require(begin(commands, &info) == VK_SUCCESS, "begin commands");
    }
    void finish() {
        require(end(commands) == VK_SUCCESS, "end commands");
        VkSubmitInfo info{}; info.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO; info.commandBufferCount = 1; info.pCommandBuffers = &commands;
        require(submit(queue, 1, &info, fence) == VK_SUCCESS && wait(device, 1, &fence, VK_TRUE, 15000000000ull) == VK_SUCCESS, "submit and complete Vulkan work");
        require(reset_fence(device, 1, &fence) == VK_SUCCESS, "reset fence");
    }
    rsf_srvk_image describe(const Image& image, VkImageLayout layout) const {
        return {device, reinterpret_cast<uint64_t>(image.image), uint32_t(image.format), image.width, image.height, uint32_t(layout), image.copy_source};
    }
    void fill(const Image& image, const std::array<float, 4>& value) {
        VkClearColorValue color{}; for (uint32_t i = 0; i < 4; ++i) color.float32[i] = value[i];
        VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        clear(commands, image.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &color, 1, &range);
    }
    void read(const Image& output) {
        transition(output, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
        VkBufferImageCopy region{}; region.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.imageExtent = {320, 180, 1};
        copy(commands, output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, readback, 1, &region);
        transition(output, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        VkBufferMemoryBarrier b{}; b.sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER; b.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; b.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
        b.srcQueueFamilyIndex = b.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED; b.buffer = readback; b.size = VK_WHOLE_SIZE;
        barrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_HOST_BIT, 0, 0, nullptr, 1, &b, 0, nullptr);
    }
    void verify(const std::array<float, 4>& expected) {
        void* pointer = nullptr; require(map(device, readback_memory, 0, VK_WHOLE_SIZE, 0, &pointer) == VK_SUCCESS, "map output");
        auto* pixels = static_cast<uint16_t*>(pointer); bool valid = true;
        for (uint32_t y = 16; y < 164; ++y) for (uint32_t x = 16; x < 304; ++x) for (uint32_t c = 0; c < 3; ++c) {
            const float value = DirectX::PackedVector::XMConvertHalfToFloat(pixels[(y * 320 + x) * 4 + c]);
            valid &= std::isfinite(value) && std::abs(value - expected[c]) < 0.025f;
        }
        unmap(device, readback_memory); require(valid, "GPU output disagrees with known linear scene colour");
    }
};

constexpr uint32_t width=33,height=17,count=width*height;
using Pixels=std::array<uint16_t,count*4>;
Pixels pattern(bool flat=false) {
    Pixels pixels{};
    for(uint32_t y=0;y<height;++y)for(uint32_t x=0;x<width;++x)for(uint32_t c=0;c<4;++c)
        pixels[(y*width+x)*4+c]=DirectX::PackedVector::XMConvertFloatToHalf(c==3?float(x)/width:flat?10.f:x<width/2?10.f+.12f*x*x:80.f+.015f*(x-width/2)*(x-width/2));
    return pixels;
}
struct Fixture {
    Gpu gpu; rsf::sharpening::Vulkan pass;
    rsf_srvk_setup setup{}; rsf_frame_record record{};
    Image output{},depth{},motion{}; bool initialized=false;
    void initialize() {
        gpu.initialize();
        output=gpu.image(width,height,VK_FORMAT_R16G16B16A16_SFLOAT,VK_IMAGE_USAGE_SAMPLED_BIT|VK_IMAGE_USAGE_TRANSFER_DST_BIT|VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
        depth=gpu.image(width,height,VK_FORMAT_R32_SFLOAT,VK_IMAGE_USAGE_SAMPLED_BIT|VK_IMAGE_USAGE_TRANSFER_DST_BIT);
        motion=gpu.image(width,height,VK_FORMAT_R16G16_SFLOAT,VK_IMAGE_USAGE_SAMPLED_BIT|VK_IMAGE_USAGE_TRANSFER_DST_BIT);
        setup.struct_size=sizeof(setup);setup.abi_version=RSF_SRVK_ABI_VERSION;setup.instance=gpu.instance;setup.physical_device=gpu.physical;setup.device=gpu.device;
        setup.instance_proc=reinterpret_cast<rsf_vk_get_proc>(gpu.gip);setup.device_proc=reinterpret_cast<rsf_vk_get_proc>(gpu.gdp);
        record.output_width=width;record.output_height=height;record.camera.depth_inverted=1;
    }
    Pixels run(const Pixels& pixels,rsf_sharpening_options options,bool with_depth=true,float velocity=0,bool depth_step=false) {
        void* mapped=nullptr; require(gpu.map(gpu.device,gpu.readback_memory,0,VK_WHOLE_SIZE,0,&mapped)==VK_SUCCESS,"map upload");
        std::memcpy(mapped,pixels.data(),sizeof(pixels)); gpu.unmap(gpu.device,gpu.readback_memory);
        gpu.start(); pass.retire(gpu.commands);
        gpu.transition(output,initialized?VK_IMAGE_LAYOUT_GENERAL:VK_IMAGE_LAYOUT_UNDEFINED,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        VkBufferImageCopy region{};region.imageSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1};region.imageExtent={width,height,1};
        gpu.device_function<PFN_vkCmdCopyBufferToImage>("vkCmdCopyBufferToImage")(gpu.commands,gpu.readback,output.image,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&region);
        gpu.transition(output,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL);
        for(const auto& image:{depth,motion})gpu.transition(image,initialized?VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL:VK_IMAGE_LAYOUT_UNDEFINED,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        gpu.fill(depth,{.5f,.5f,.5f,.5f});gpu.fill(motion,{velocity,0,0,0});
        if(depth_step) {
            // Clear each side independently through a tiny upload after the colour upload.
            require(gpu.end(gpu.commands)==VK_SUCCESS,"finish upload section");
            VkSubmitInfo info{VK_STRUCTURE_TYPE_SUBMIT_INFO};info.commandBufferCount=1;info.pCommandBuffers=&gpu.commands;
            require(gpu.submit(gpu.queue,1,&info,gpu.fence)==VK_SUCCESS&&gpu.wait(gpu.device,1,&gpu.fence,VK_TRUE,15000000000ull)==VK_SUCCESS,"complete colour upload");
            require(gpu.reset_fence(gpu.device,1,&gpu.fence)==VK_SUCCESS,"reset upload fence");
            require(gpu.map(gpu.device,gpu.readback_memory,0,VK_WHOLE_SIZE,0,&mapped)==VK_SUCCESS,"map depth upload");
            for(uint32_t i=0;i<count;++i)static_cast<float*>(mapped)[i]=i%width<width/2?.1f:.9f;
            gpu.unmap(gpu.device,gpu.readback_memory);gpu.start();
            gpu.device_function<PFN_vkCmdCopyBufferToImage>("vkCmdCopyBufferToImage")(gpu.commands,gpu.readback,depth.image,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&region);
        }
        for(const auto& image:{depth,motion})gpu.transition(image,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        rsf_srvk_frame frame{};frame.record=&record;frame.sharpening=options;frame.pre_exposure=1;
        frame.motion=gpu.describe(motion,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);frame.motion_scale_x=frame.motion_scale_y=1;
        if(with_depth)frame.depth=gpu.describe(depth,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        require(pass.run(setup,gpu.commands,frame,output.image),"record Vulkan sharpening");
        gpu.transition(output,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
        VkBufferMemoryBarrier ordering{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};ordering.srcAccessMask=VK_ACCESS_TRANSFER_READ_BIT;ordering.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;
        ordering.srcQueueFamilyIndex=ordering.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;ordering.buffer=gpu.readback;ordering.size=VK_WHOLE_SIZE;
        gpu.barrier(gpu.commands,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,1,&ordering,0,nullptr);
        gpu.copy(gpu.commands,output.image,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,gpu.readback,1,&region);
        gpu.transition(output,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL);gpu.finish();initialized=true;
        require(gpu.map(gpu.device,gpu.readback_memory,0,VK_WHOLE_SIZE,0,&mapped)==VK_SUCCESS,"map result");Pixels result{};std::memcpy(result.data(),mapped,sizeof(result));gpu.unmap(gpu.device,gpu.readback_memory);return result;
    }
};
double delta(const Pixels& a,const Pixels& b) { double value=0;for(uint32_t i=0;i<count*4;++i)if(i%4!=3)value+=std::abs(DirectX::PackedVector::XMConvertHalfToFloat(a[i])-DirectX::PackedVector::XMConvertHalfToFloat(b[i]));return value; }
void in_flight_case() {
    Fixture f;f.initialize();auto& g=f.gpu;const auto source=pattern();
    std::array<Image,3> images{};std::array<VkCommandBuffer,3> commands{};
    for(auto& image:images)image=g.image(width,height,VK_FORMAT_R16G16B16A16_SFLOAT,VK_IMAGE_USAGE_SAMPLED_BIT|VK_IMAGE_USAGE_TRANSFER_DST_BIT|VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
    void* mapped=nullptr;require(g.map(g.device,g.readback_memory,0,VK_WHOLE_SIZE,0,&mapped)==VK_SUCCESS,"map multi-frame upload");
    for(uint32_t i=0;i<3;++i)std::memcpy(static_cast<char*>(mapped)+i*sizeof(source),source.data(),sizeof(source));g.unmap(g.device,g.readback_memory);
    VkCommandBufferAllocateInfo allocation{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};allocation.commandPool=g.pool;allocation.commandBufferCount=3;
    require(g.allocate_commands(g.device,&allocation,commands.data())==VK_SUCCESS,"allocate three in-flight command buffers");
    for(uint32_t i=0;i<3;++i) {
        g.commands=commands[i];VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};require(g.begin(g.commands,&begin)==VK_SUCCESS,"begin in-flight recording");
        g.transition(images[i],VK_IMAGE_LAYOUT_UNDEFINED,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        VkBufferImageCopy region{};region.bufferOffset=i*sizeof(source);region.imageSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1};region.imageExtent={width,height,1};
        g.device_function<PFN_vkCmdCopyBufferToImage>("vkCmdCopyBufferToImage")(g.commands,g.readback,images[i].image,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&region);
        g.transition(images[i],VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,VK_IMAGE_LAYOUT_GENERAL);
        rsf_srvk_frame frame{};frame.record=&f.record;frame.pre_exposure=1;frame.sharpening=rsf_sharpening_defaults();frame.sharpening.mode=1;frame.sharpening.sharpness=.2f+.4f*i;
        require(f.pass.run(f.setup,g.commands,frame,images[i].image),"record independent constants/descriptors before any submit");
        g.transition(images[i],VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
        VkBufferMemoryBarrier order{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};order.srcAccessMask=VK_ACCESS_TRANSFER_READ_BIT;order.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;
        order.srcQueueFamilyIndex=order.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;order.buffer=g.readback;order.offset=region.bufferOffset;order.size=sizeof(source);
        g.barrier(g.commands,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,1,&order,0,nullptr);
        g.copy(g.commands,images[i].image,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,g.readback,1,&region);
        require(g.end(g.commands)==VK_SUCCESS,"end in-flight recording");
    }
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};submit.commandBufferCount=3;submit.pCommandBuffers=commands.data();
    require(g.submit(g.queue,1,&submit,g.fence)==VK_SUCCESS&&g.wait(g.device,1,&g.fence,VK_TRUE,15000000000ull)==VK_SUCCESS,"submit three recordings together");
    require(g.map(g.device,g.readback_memory,0,VK_WHOLE_SIZE,0,&mapped)==VK_SUCCESS,"map in-flight results");
    double deltas[3]{};for(uint32_t i=0;i<3;++i){Pixels actual{};std::memcpy(actual.data(),static_cast<char*>(mapped)+i*sizeof(source),sizeof(source));deltas[i]=delta(source,actual);}g.unmap(g.device,g.readback_memory);
    require(deltas[0]>0&&deltas[1]>deltas[0]&&deltas[2]>deltas[1],"in-flight constants cannot be overwritten by later recordings");
    require(g.reset_pool(g.device,g.pool,0)==VK_SUCCESS,"reset completed recording pool");for(auto command:commands)f.pass.retire(command);
    std::printf("three in-flight UBO/descriptor slots: %.6f %.6f %.6f\\n",deltas[0],deltas[1],deltas[2]);
}

}
int main() try {
    rsf::sharpening::Vulkan disabled; rsf_srvk_frame empty{};
    require(disabled.run({},nullptr,empty,VK_NULL_HANDLE)&&disabled.dispatches()==0,"Off requires no device and records no work");
    in_flight_case();
    Fixture f;f.initialize();auto options=rsf_sharpening_defaults();const auto source=pattern();
    require(f.run(source,options)==source&&f.pass.dispatches()==0,"Off bit identical and no dispatch");
    options.mode=4;require(f.run(source,options)==source&&f.pass.dispatches()==0,"vendor no custom pass");
    options.mode=1;options.sharpness=0;require(f.run(source,options)==source&&f.pass.dispatches()==0,"zero no dispatch");
    for(uint32_t mode=1;mode<=3;++mode) {
        options=rsf_sharpening_defaults();options.mode=mode;options.sharpness=1;
        require(f.run(pattern(true),options)==pattern(true),"flat byte identity and alpha");
        auto actual=f.run(source,options);require(delta(source,actual)>0,"edge sharpened");
        for(uint32_t i=0;i<count;++i) {
            require(actual[i*4+3]==source[i*4+3],"alpha survives");
            float lower=1e10f,upper=0;
            for(int dy=-1;dy<=1;++dy)for(int dx=-1;dx<=1;++dx) {
                const int x=std::clamp(int(i%width)+dx,0,int(width)-1),y=std::clamp(int(i/width)+dy,0,int(height)-1);
                const float value=DirectX::PackedVector::XMConvertHalfToFloat(source[(y*width+x)*4]);lower=std::min(lower,value);upper=std::max(upper,value);
            }
            const float value=DirectX::PackedVector::XMConvertHalfToFloat(actual[i*4]);require(std::isfinite(value)&&value>=lower-.07f&&value<=upper+.07f,"HDR local bounds");
        }
        auto fallback=f.run(source,options,false);auto plain=options;plain.mode=1;require(fallback==f.run(source,plain,false),"missing depth RCAS fallback");
        options.sharpness=.2f;auto low=f.run(source,options);options.sharpness=.6f;auto medium=f.run(source,options);options.sharpness=1;auto high=f.run(source,options);
        std::printf("measured mode %u: %.6f %.6f %.6f\n",mode,delta(source,low),delta(source,medium),delta(source,high)); require(delta(source,high)>0&&delta(source,medium)>=delta(source,low)&&delta(source,high)>=delta(source,medium),"monotonic sharpness");
        std::printf("mode %u edge deltas %.6f %.6f %.6f\n",mode,delta(source,low),delta(source,medium),delta(source,high));
    }
    for(uint32_t mode:{2u,3u}) { options=rsf_sharpening_defaults();options.mode=mode;options.clamp_output=1;
        const auto clipped=f.run(pattern(true),options);for(uint32_t i=0;i<count;++i)require(DirectX::PackedVector::XMConvertHalfToFloat(clipped[i*4])==1,"explicit clamp limits decoded HDR even on flat pixels"); }
    options=rsf_sharpening_defaults();options.mode=1;options.sharpness=.2f;options.mas=1;options.motion_threshold=2;options.motion_scale_limit=10;options.motion_sharpness=.6f;
    auto still=f.run(source,options,true,0),below=f.run(source,options,true,1),moving=f.run(source,options,true,10);
    require(still==below&&delta(source,moving)>delta(source,still),"MAS threshold and ramp");
    for(uint32_t mode:{2u,3u}) {options=rsf_sharpening_defaults();options.mode=mode;options.sharpness=1;auto normal=f.run(source,options);auto step=f.run(source,options,true,0,true);double a=0,b=0;for(uint32_t y=0;y<height;++y)for(uint32_t x:{width/2-1,width/2}){uint32_t i=(y*width+x)*4;a+=std::abs(DirectX::PackedVector::XMConvertHalfToFloat(normal[i])-DirectX::PackedVector::XMConvertHalfToFloat(source[i]));b+=std::abs(DirectX::PackedVector::XMConvertHalfToFloat(step[i])-DirectX::PackedVector::XMConvertHalfToFloat(source[i]));} std::printf("depth mode %u boundary deltas %.6f %.6f\n",mode,a,b);require(b==0,"depth edge rejects cross-depth sharpening");}
    std::printf("sharpening_vulkan: PASS, %llu dispatches, explicit command-buffer retirement\n",static_cast<unsigned long long>(f.pass.dispatches()));return 0;
} catch(const std::exception& error) {if(!std::strcmp(error.what(),"Vulkan loader")||!std::strcmp(error.what(),"enumerate GPUs")){std::printf("SKIP: no Vulkan device (%s)\n",error.what());return 77;}std::printf("FAIL: %s\n",error.what());return 1;}
