// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <cmath>
#include <vector>

namespace {
void require(bool value, const char* message) { if (!value) throw std::runtime_error(message); }
}

#if defined(RSF_TEST_VULKAN)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include <DirectXPackedVector.h>
#include <array>
#include <fstream>
#include <filesystem>

namespace {
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
    void initialize() {
        module = LoadLibraryExW(L"vulkan-1.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32); require(module != nullptr, "Vulkan loader");
        gip = reinterpret_cast<PFN_vkGetInstanceProcAddr>(GetProcAddress(module, "vkGetInstanceProcAddr")); require(gip != nullptr, "Vulkan instance dispatch");
        VkApplicationInfo app{}; app.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO; app.pApplicationName = "ReScaleFrame Vulkan SR fixture"; app.apiVersion = VK_API_VERSION_1_2;
        VkInstanceCreateInfo instance_info{}; instance_info.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO; instance_info.pApplicationInfo = &app;
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
        VkBufferCreateInfo buffer{}; buffer.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO; buffer.size = 320 * 180 * 8; buffer.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
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
void device_case(Gpu& gpu, const char* directory, uint32_t backend) {
    rsf_srvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
    setup.instance = gpu.instance; setup.physical_device = gpu.physical; setup.device = gpu.device;
    setup.instance_proc = reinterpret_cast<rsf_vk_get_proc>(gpu.gip); setup.device_proc = reinterpret_cast<rsf_vk_get_proc>(gpu.gdp);
    setup.backend = backend; setup.quality = 1; setup.output_width = 320; setup.output_height = 180;
    setup.depth_inverted = 1; setup.runtime_directory_utf8 = directory;
    rsf_srvk* context = nullptr; auto result = rsf_srvk_create(&setup, &context);
    std::printf("Vulkan SR backend %u create=%d\n", backend, result); require(result == RSF_BACKEND_OK && context, "create native Vulkan SR context");
    // Context retirement must precede device retirement even when an assertion fails.
    struct ContextOwner { Gpu& gpu; rsf_srvk* value; ~ContextOwner() { (void)gpu.idle(gpu.device); (void)rsf_srvk_destroy(value); } } owner{gpu, context};
    uint32_t width = 0, height = 0; require(rsf_srvk_plan(context, &width, &height) == RSF_BACKEND_OK && width && height, "SR sizing plan");
    auto color = gpu.image(width, height, VK_FORMAT_R16G16B16A16_SFLOAT, VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT);
    auto depth = gpu.image(width, height, VK_FORMAT_R32_SFLOAT, VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT);
    auto motion = gpu.image(width, height, VK_FORMAT_R16G16_SFLOAT, VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT);
    auto output = gpu.image(320, 180, VK_FORMAT_R16G16B16A16_SFLOAT, VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 43; record.view_id = 1; record.resource_generation = 1; record.frame_time_ms = 16.667f;
    record.render_width = record.camera.render_width = width; record.render_height = record.camera.render_height = height;
    record.output_width = record.camera.output_width = 320; record.output_height = record.camera.output_height = 180;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.depth_inverted = 1; record.camera.near_plane = 0.1f; record.camera.far_plane = 100; record.camera.vertical_fov_radians = 1;
    rsf_srvk_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = RSF_SRVK_ABI_VERSION; frame.record = &record;
    frame.color = gpu.describe(color, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL); frame.depth = gpu.describe(depth, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    frame.motion = gpu.describe(motion, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL); frame.output = gpu.describe(output, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    frame.motion_scale_x = frame.motion_scale_y = frame.pre_exposure = 1;
    for (uint32_t index = 1; index <= 12; ++index) {
        const std::array<float, 4> value = index < 7 ? std::array<float, 4>{0.18f, 0.05f, 0.7f, 1} : std::array<float, 4>{0.05f, 0.4f, 0.12f, 1};
        gpu.start();
        for (const auto* image : {&color, &depth, &motion}) gpu.transition(*image, index == 1 ? VK_IMAGE_LAYOUT_UNDEFINED : VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        if (index == 1) gpu.transition(output, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        gpu.fill(color, value); gpu.fill(depth, {0.5f, 0, 0, 0}); gpu.fill(motion, {0, 0, 0, 0});
        for (const auto* image : {&color, &depth, &motion}) gpu.transition(*image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        record.frame_id = index; record.flags = index == 1 || index == 7 ? RSF_FRAME_FLAG_RESET : 0;
        auto invalid = frame; invalid.color.device = nullptr;
        require(rsf_srvk_evaluate(context, gpu.commands, &invalid) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "foreign device image refusal");
        result = rsf_srvk_evaluate(context, gpu.commands, &frame);
        std::printf("backend=%u frame=%u dispatch=%d reset=%u\n", backend, index, result, record.flags);
        require(result == RSF_BACKEND_OK, "Vulkan SR dispatch");
        gpu.read(output); gpu.finish(); gpu.verify(value);
    }
    require(gpu.idle(gpu.device) == VK_SUCCESS && rsf_srvk_destroy(context) == RSF_BACKEND_OK, "drain and retire Vulkan SR"); owner.value = nullptr;
    std::printf("PASS: Vulkan FSR%u GPU pixels, reset and image-owner guards\n", backend);
}
}
#endif

int main(int argc, char**) try {
    rsf_fgvk* fg = reinterpret_cast<rsf_fgvk*>(1); uint64_t chain = 1;
    require(rsf_fgvk_create(nullptr, &fg, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !fg && !chain, "FG null setup refusal");
    rsf_fgvk_setup short_fg{}; short_fg.struct_size = sizeof(short_fg) - 1;
    require(rsf_fgvk_create(&short_fg, &fg, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "FG short setup refusal");
    short_fg.struct_size = sizeof(short_fg); short_fg.abi_version = RSF_FGVK_ABI_VERSION + 1;
    require(rsf_fgvk_create(&short_fg, &fg, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH, "FG ABI mismatch refusal");
    short_fg.abi_version = RSF_FGVK_ABI_VERSION; short_fg.graphics.struct_size = sizeof(short_fg.graphics) - 1;
    require(rsf_fgvk_create(&short_fg, &fg, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "FG embedded graphics size refusal");
    short_fg.graphics.struct_size = sizeof(short_fg.graphics); short_fg.graphics.abi_version = RSF_SRVK_ABI_VERSION + 1;
    require(rsf_fgvk_create(&short_fg, &fg, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH, "FG embedded graphics ABI refusal");
    require(rsf_fgvk_destroy(nullptr) == RSF_BACKEND_OK && rsf_fgvk_drain(nullptr) == RSF_BACKEND_OK &&
        rsf_fgvk_prepare(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "FG null lifecycle contract");
    rsf_srvk* context = reinterpret_cast<rsf_srvk*>(1);
    require(rsf_srvk_create(nullptr, &context) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !context, "null setup refusal");
    rsf_srvk_setup short_setup{}; short_setup.struct_size = sizeof(short_setup) - 1;
    require(rsf_srvk_create(&short_setup, &context) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !context, "short setup refusal");
    short_setup.struct_size = sizeof(short_setup); short_setup.abi_version = RSF_SRVK_ABI_VERSION + 1;
    require(rsf_srvk_create(&short_setup, &context) == RSF_BACKEND_ERROR_ABI_MISMATCH, "ABI mismatch refusal");
    require(rsf_srvk_plan(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        rsf_srvk_evaluate(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && rsf_srvk_destroy(nullptr) == RSF_BACKEND_OK, "null lifecycle contract");
    if (argc == 1) { std::puts("PASS: native Vulkan SR C ABI guards"); return 0; }
    const char* directory = std::getenv("RSF_VULKAN_SR_RUNTIME");
    if (!directory) { std::puts("SKIP: set RSF_VULKAN_SR_RUNTIME for the Vulkan device fixture"); return 77; }
#if defined(RSF_TEST_VULKAN)
    Gpu gpu; gpu.initialize(); device_case(gpu, directory, RSF_SR_FSR2); device_case(gpu, directory, RSF_SR_FSR3); return 0;
#else
    std::puts("SKIP: built without Vulkan SDK headers"); return 77;
#endif
} catch (const std::exception& error) { std::fprintf(stderr, "%s\n", error.what()); return 1; }
