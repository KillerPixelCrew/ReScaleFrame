// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cstring>
#include <rescaleframe/native_sr_vulkan.h>

namespace rsf_fixture {
inline PFN_vkGetInstanceProcAddr instance_proc{};
inline PFN_vkGetDeviceProcAddr device_proc{};
inline rsf_vk_function instance_function(void* object, const char* name) {
    return reinterpret_cast<rsf_vk_function>(instance_proc(static_cast<VkInstance>(object), name));
}
inline rsf_vk_function device_function(void* object, const char* name) {
    return reinterpret_cast<rsf_vk_function>(device_proc(static_cast<VkDevice>(object), name));
}
struct Device {
    HMODULE loader{};
    VkInstance instance{};
    VkPhysicalDevice physical{};
    VkDevice device{};
    VkQueue queue{};
    uint32_t family{};
    VkPhysicalDeviceProperties properties{};
    template<typename T> T function(const char* name) const { return reinterpret_cast<T>(device_proc(device, name)); }
    int init() {
        loader = LoadLibraryW(L"vulkan-1.dll");
        if (!loader) return 10;
        instance_proc = reinterpret_cast<PFN_vkGetInstanceProcAddr>(GetProcAddress(loader, "vkGetInstanceProcAddr"));
        if (!instance_proc) return 11;
        VkApplicationInfo application{VK_STRUCTURE_TYPE_APPLICATION_INFO}; application.apiVersion = VK_API_VERSION_1_2;
        application.pApplicationName = "ReScaleFrame interop device fixture";
        VkInstanceCreateInfo create{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO}; create.pApplicationInfo = &application;
        auto createInstance = reinterpret_cast<PFN_vkCreateInstance>(instance_proc(nullptr, "vkCreateInstance"));
        int result = createInstance(&create, nullptr, &instance);
        if (result) return result;
        auto enumerate = reinterpret_cast<PFN_vkEnumeratePhysicalDevices>(instance_proc(instance, "vkEnumeratePhysicalDevices"));
        uint32_t count = 16; std::array<VkPhysicalDevice, 16> devices{};
        result = enumerate(instance, &count, devices.data());
        if (result || !count) return result ? result : 12;
        physical = devices[0];
        reinterpret_cast<PFN_vkGetPhysicalDeviceProperties>(instance_proc(instance, "vkGetPhysicalDeviceProperties"))(physical, &properties);
        auto queues = reinterpret_cast<PFN_vkGetPhysicalDeviceQueueFamilyProperties>(instance_proc(instance, "vkGetPhysicalDeviceQueueFamilyProperties"));
        count = 32; std::array<VkQueueFamilyProperties, 32> families{}; queues(physical, &count, families.data());
        while (family < count && !(families[family].queueFlags & VK_QUEUE_GRAPHICS_BIT)) ++family;
        if (family == count) return 13;
        float priority = 1;
        VkDeviceQueueCreateInfo queueInfo{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
        queueInfo.queueFamilyIndex = family; queueInfo.queueCount = 1; queueInfo.pQueuePriorities = &priority;
        const char* extensions[] = {VK_KHR_EXTERNAL_MEMORY_WIN32_EXTENSION_NAME, VK_KHR_EXTERNAL_SEMAPHORE_WIN32_EXTENSION_NAME};
        VkPhysicalDeviceTimelineSemaphoreFeatures timeline{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES}; timeline.timelineSemaphore = VK_TRUE;
        VkDeviceCreateInfo deviceInfo{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO}; deviceInfo.pNext = &timeline;
        deviceInfo.queueCreateInfoCount = 1; deviceInfo.pQueueCreateInfos = &queueInfo;
        deviceInfo.enabledExtensionCount = 2; deviceInfo.ppEnabledExtensionNames = extensions;
        auto createDevice = reinterpret_cast<PFN_vkCreateDevice>(instance_proc(instance, "vkCreateDevice"));
        result = createDevice(physical, &deviceInfo, nullptr, &device);
        if (result) return result;
        device_proc = reinterpret_cast<PFN_vkGetDeviceProcAddr>(instance_proc(instance, "vkGetDeviceProcAddr"));
        function<PFN_vkGetDeviceQueue>("vkGetDeviceQueue")(device, family, 0, &queue);
        return 0;
    }
    void close() {
        if (device) function<PFN_vkDestroyDevice>("vkDestroyDevice")(device, nullptr);
        if (instance) reinterpret_cast<PFN_vkDestroyInstance>(instance_proc(instance, "vkDestroyInstance"))(instance, nullptr);
        if (loader) FreeLibrary(loader);
    }
    rsf_srvk_setup setup() const {
        rsf_srvk_setup result{}; result.struct_size = sizeof(result); result.abi_version = RSF_SRVK_ABI_VERSION;
        result.instance = instance; result.physical_device = physical; result.device = device;
        result.instance_proc = instance_function; result.device_proc = device_function;
        return result;
    }
};
struct Image {
    VkImage image{}; VkDeviceMemory memory{}; uint32_t format{}, width{}, height{};
    int init(const Device& owner, uint32_t requested_format, uint32_t w, uint32_t h) {
        format = requested_format; width = w; height = h;
        VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
        create.imageType = VK_IMAGE_TYPE_2D; create.format = static_cast<VkFormat>(format); create.extent = {w, h, 1};
        create.mipLevels = create.arrayLayers = 1; create.samples = VK_SAMPLE_COUNT_1_BIT; create.tiling = VK_IMAGE_TILING_OPTIMAL;
        create.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
        int result = owner.function<PFN_vkCreateImage>("vkCreateImage")(owner.device, &create, nullptr, &image);
        if (result) return result;
        VkMemoryRequirements requirements{};
        owner.function<PFN_vkGetImageMemoryRequirements>("vkGetImageMemoryRequirements")(owner.device, image, &requirements);
        VkPhysicalDeviceMemoryProperties memoryProperties{};
        reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(instance_proc(owner.instance, "vkGetPhysicalDeviceMemoryProperties"))(owner.physical, &memoryProperties);
        VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocate.allocationSize = requirements.size;
        allocate.memoryTypeIndex = memoryProperties.memoryTypeCount;
        for (uint32_t i = 0; i < memoryProperties.memoryTypeCount; ++i)
            if ((requirements.memoryTypeBits & (1u << i)) && (memoryProperties.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) {
                allocate.memoryTypeIndex = i; break;
            }
        if (allocate.memoryTypeIndex == memoryProperties.memoryTypeCount) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
        result = owner.function<PFN_vkAllocateMemory>("vkAllocateMemory")(owner.device, &allocate, nullptr, &memory);
        return result ? result : owner.function<PFN_vkBindImageMemory>("vkBindImageMemory")(owner.device, image, memory, 0);
    }
    void close(const Device& owner) {
        if (image) owner.function<PFN_vkDestroyImage>("vkDestroyImage")(owner.device, image, nullptr);
        if (memory) owner.function<PFN_vkFreeMemory>("vkFreeMemory")(owner.device, memory, nullptr);
    }
    rsf_srvk_image input(const Device& owner, uint32_t layout) const {
        return {owner.device, reinterpret_cast<uint64_t>(image), format, width, height, layout, 1};
    }
};
}
