// SPDX-License-Identifier: GPL-3.0-only
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cstring>
#include <new>
#include "native_fg_xess_vulkan.h"

struct ID3D12Resource;
extern "C" int VulkanStoryXessFgCreateSharedImage(rsf_xess_bridge*, uint32_t,
    uint32_t, uint32_t, HANDLE*, ID3D12Resource**);
extern "C" void VulkanStoryXessFgReleaseImage(ID3D12Resource*);
extern "C" int VulkanStoryXessFgCreateSharedFence(rsf_xess_bridge*, HANDLE*);

struct rsf_xessvk_interop {
    VkPhysicalDevice physical{};
    VkDevice device{};
    rsf_xess_bridge* bridge{};
    VkSemaphore timeline{};
    struct Image { VkImage image{}; VkDeviceMemory memory{}; ID3D12Resource* dx12{}; };
    std::array<Image, 24> images{}; // Six frame slots, four distinct shared inputs.
    PFN_vkGetPhysicalDeviceImageFormatProperties2 imageProperties{};
    PFN_vkGetPhysicalDeviceMemoryProperties memoryProperties{};
    PFN_vkCreateImage createImage{};
    PFN_vkDestroyImage destroyImage{};
    PFN_vkGetImageMemoryRequirements imageRequirements{};
    PFN_vkGetMemoryWin32HandlePropertiesKHR handleProperties{};
    PFN_vkAllocateMemory allocateMemory{};
    PFN_vkFreeMemory freeMemory{};
    PFN_vkBindImageMemory bindImage{};
    PFN_vkCreateSemaphore createSemaphore{};
    PFN_vkDestroySemaphore destroySemaphore{};
    PFN_vkImportSemaphoreWin32HandleKHR importSemaphore{};
};

int32_t rsf_xessvk_interop_destroy(rsf_xessvk_interop* owner, uint32_t queues_drained) {
    if (!owner) return 0;
    if (queues_drained != 1) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    for (auto& image : owner->images) {
        if (image.image) owner->destroyImage(owner->device, image.image, nullptr);
        if (image.memory) owner->freeMemory(owner->device, image.memory, nullptr);
        if (image.dx12) VulkanStoryXessFgReleaseImage(image.dx12);
    }
    if (owner->timeline) owner->destroySemaphore(owner->device, owner->timeline, nullptr);
    delete owner;
    return 0;
}

int32_t rsf_xessvk_interop_create(const rsf_srvk_setup* setup, rsf_xess_bridge* bridge,
    uint32_t timeline_enabled, rsf_xessvk_interop** output) {
    if (!output) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    *output = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || !bridge || timeline_enabled != 1 ||
        !setup->instance || !setup->physical_device || !setup->device ||
        !setup->instance_proc || !setup->device_proc) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION) return RSF_XESS_BRIDGE_ABI_MISMATCH;
    auto getProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceProperties2"));
    if (!getProperties) return VK_ERROR_EXTENSION_NOT_PRESENT;
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 deviceProperties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
    deviceProperties.pNext = &identity;
    getProperties(static_cast<VkPhysicalDevice>(setup->physical_device), &deviceProperties);
    uint64_t luid = 0;
    static_assert(sizeof(luid) == VK_LUID_SIZE);
    std::memcpy(&luid, identity.deviceLUID, sizeof(luid));
    if (!identity.deviceLUIDValid || !luid || luid != RsfXessBridgeAdapterLuid(bridge))
        return VK_ERROR_INVALID_EXTERNAL_HANDLE;
    auto* owner = new (std::nothrow) rsf_xessvk_interop{};
    if (!owner) return VK_ERROR_OUT_OF_HOST_MEMORY;
    owner->physical = static_cast<VkPhysicalDevice>(setup->physical_device);
    owner->device = static_cast<VkDevice>(setup->device);
    owner->bridge = bridge;
#define INSTANCE(field, name) owner->field = reinterpret_cast<PFN_##name>(setup->instance_proc(setup->instance, #name))
#define DEVICE(field, name) owner->field = reinterpret_cast<PFN_##name>(setup->device_proc(setup->device, #name))
    INSTANCE(imageProperties, vkGetPhysicalDeviceImageFormatProperties2);
    INSTANCE(memoryProperties, vkGetPhysicalDeviceMemoryProperties);
    auto semaphoreProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceExternalSemaphoreProperties>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceExternalSemaphoreProperties"));
    DEVICE(createImage, vkCreateImage);
    DEVICE(destroyImage, vkDestroyImage);
    DEVICE(imageRequirements, vkGetImageMemoryRequirements);
    DEVICE(handleProperties, vkGetMemoryWin32HandlePropertiesKHR);
    DEVICE(allocateMemory, vkAllocateMemory);
    DEVICE(freeMemory, vkFreeMemory);
    DEVICE(bindImage, vkBindImageMemory);
    DEVICE(createSemaphore, vkCreateSemaphore);
    DEVICE(destroySemaphore, vkDestroySemaphore);
    DEVICE(importSemaphore, vkImportSemaphoreWin32HandleKHR);
#undef INSTANCE
#undef DEVICE
    if (!owner->imageProperties || !owner->memoryProperties || !semaphoreProperties ||
        !owner->createImage || !owner->destroyImage || !owner->imageRequirements ||
        !owner->handleProperties || !owner->allocateMemory || !owner->freeMemory ||
        !owner->bindImage || !owner->createSemaphore || !owner->destroySemaphore ||
        !owner->importSemaphore) { delete owner; return VK_ERROR_EXTENSION_NOT_PRESENT; }
    VkSemaphoreTypeCreateInfo type{VK_STRUCTURE_TYPE_SEMAPHORE_TYPE_CREATE_INFO};
    type.semaphoreType = VK_SEMAPHORE_TYPE_TIMELINE;
    VkPhysicalDeviceExternalSemaphoreInfo query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_SEMAPHORE_INFO};
    query.pNext = &type;
    query.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT;
    VkExternalSemaphoreProperties properties{VK_STRUCTURE_TYPE_EXTERNAL_SEMAPHORE_PROPERTIES};
    semaphoreProperties(owner->physical, &query, &properties);
    if (!(properties.externalSemaphoreFeatures & VK_EXTERNAL_SEMAPHORE_FEATURE_IMPORTABLE_BIT)) {
        delete owner; return VK_ERROR_INVALID_EXTERNAL_HANDLE;
    }
    VkSemaphoreCreateInfo create{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    create.pNext = &type;
    VkResult result = owner->createSemaphore(owner->device, &create, nullptr, &owner->timeline);
    HANDLE handle = nullptr;
    if (result == VK_SUCCESS) {
        int32_t bridge_result = VulkanStoryXessFgCreateSharedFence(bridge, &handle);
        if (bridge_result) { rsf_xessvk_interop_destroy(owner, 1); return bridge_result; }
        VkImportSemaphoreWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_SEMAPHORE_WIN32_HANDLE_INFO_KHR};
        import.semaphore = owner->timeline;
        import.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT;
        import.handle = handle; // Permanent reference import; NT handle remains ours.
        result = owner->importSemaphore(owner->device, &import);
    }
    if (handle) CloseHandle(handle);
    if (result != VK_SUCCESS) { rsf_xessvk_interop_destroy(owner, 1); return result; }
    *output = owner;
    return 0;
}

uint64_t rsf_xessvk_interop_timeline(const rsf_xessvk_interop* owner) {
    return owner ? reinterpret_cast<uint64_t>(owner->timeline) : 0;
}

int32_t rsf_xessvk_interop_image(rsf_xessvk_interop* owner, uint32_t format,
    uint32_t width, uint32_t height, rsf_xessvk_shared_image* output) {
    if (!output) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    *output = {};
    if (!owner || !width || !height || (format != VK_FORMAT_B8G8R8A8_UNORM &&
        format != VK_FORMAT_R32_SFLOAT && format != VK_FORMAT_R16G16_SFLOAT))
        return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    rsf_xessvk_interop::Image* slot = nullptr;
    for (auto& image : owner->images) if (!image.image && !image.dx12) { slot = &image; break; }
    if (!slot) return VK_ERROR_TOO_MANY_OBJECTS;
    VkPhysicalDeviceExternalImageFormatInfo externalQuery{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_IMAGE_FORMAT_INFO};
    externalQuery.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkPhysicalDeviceImageFormatInfo2 query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_IMAGE_FORMAT_INFO_2};
    query.pNext = &externalQuery;
    query.format = static_cast<VkFormat>(format);
    query.type = VK_IMAGE_TYPE_2D;
    query.tiling = VK_IMAGE_TILING_OPTIMAL;
    query.usage = VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    VkExternalImageFormatProperties external{VK_STRUCTURE_TYPE_EXTERNAL_IMAGE_FORMAT_PROPERTIES};
    VkImageFormatProperties2 properties{VK_STRUCTURE_TYPE_IMAGE_FORMAT_PROPERTIES_2};
    properties.pNext = &external;
    VkResult result = owner->imageProperties(owner->physical, &query, &properties);
    if (result != VK_SUCCESS) return result;
    if (!(external.externalMemoryProperties.externalMemoryFeatures & VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT) ||
        width > properties.imageFormatProperties.maxExtent.width ||
        height > properties.imageFormatProperties.maxExtent.height) return VK_ERROR_FORMAT_NOT_SUPPORTED;
    HANDLE handle = nullptr;
    int32_t bridge_result = VulkanStoryXessFgCreateSharedImage(owner->bridge, width, height,
        format, &handle, &slot->dx12);
    if (bridge_result) return bridge_result;
    VkExternalMemoryImageCreateInfo externalCreate{VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    externalCreate.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    create.pNext = &externalCreate;
    create.imageType = query.type;
    create.format = query.format;
    create.extent = {width, height, 1};
    create.mipLevels = create.arrayLayers = 1;
    create.samples = VK_SAMPLE_COUNT_1_BIT;
    create.tiling = query.tiling;
    create.usage = query.usage;
    result = owner->createImage(owner->device, &create, nullptr, &slot->image);
    VkMemoryRequirements requirements{};
    VkMemoryWin32HandlePropertiesKHR handleProperties{VK_STRUCTURE_TYPE_MEMORY_WIN32_HANDLE_PROPERTIES_KHR};
    if (result == VK_SUCCESS) {
        owner->imageRequirements(owner->device, slot->image, &requirements);
        result = owner->handleProperties(owner->device,
            VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT, handle, &handleProperties);
    }
    if (result == VK_SUCCESS) {
        VkPhysicalDeviceMemoryProperties memory{};
        owner->memoryProperties(owner->physical, &memory);
        uint32_t index = memory.memoryTypeCount;
        for (uint32_t i = 0; i < memory.memoryTypeCount; ++i)
            if ((requirements.memoryTypeBits & handleProperties.memoryTypeBits & (1u << i)) &&
                (memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { index = i; break; }
        if (index == memory.memoryTypeCount) result = VK_ERROR_INCOMPATIBLE_DRIVER;
        else {
            VkMemoryDedicatedAllocateInfo dedicated{VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO};
            dedicated.image = slot->image;
            VkImportMemoryWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR};
            import.pNext = &dedicated;
            import.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
            import.handle = handle;
            VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
            allocate.pNext = &import;
            allocate.allocationSize = requirements.size;
            allocate.memoryTypeIndex = index;
            result = owner->allocateMemory(owner->device, &allocate, nullptr, &slot->memory);
            if (result == VK_SUCCESS) result = owner->bindImage(owner->device, slot->image, slot->memory, 0);
        }
    }
    CloseHandle(handle); // Win32 imports retain a payload reference, not this NT handle.
    if (result != VK_SUCCESS) {
        if (slot->image) owner->destroyImage(owner->device, slot->image, nullptr);
        if (slot->memory) owner->freeMemory(owner->device, slot->memory, nullptr);
        VulkanStoryXessFgReleaseImage(slot->dx12);
        *slot = {};
        return result;
    }
    *output = {reinterpret_cast<uint64_t>(slot->image), slot->dx12, format, width, height};
    return 0;
}
