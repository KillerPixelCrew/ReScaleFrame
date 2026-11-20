// SPDX-License-Identifier: MIT
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <cstring>
#include <new>
#include "native_dx12_vulkan_platform.h"

namespace dx12vk_platform {
static_assert(VK_FORMAT_B8G8R8A8_UNORM == 44 && VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R32_SFLOAT == 100);
static_assert(VK_ERROR_OUT_OF_HOST_MEMORY == -1 && VK_ERROR_INITIALIZATION_FAILED == -3 &&
    VK_ERROR_INCOMPATIBLE_DRIVER == -9 && VK_ERROR_TOO_MANY_OBJECTS == -10 &&
    VK_ERROR_FORMAT_NOT_SUPPORTED == -11 && VK_ERROR_INVALID_EXTERNAL_HANDLE == -1000072003);

struct Dispatch {
    VkPhysicalDevice physical{};
    VkDevice device{};
    PFN_vkGetPhysicalDeviceImageFormatProperties2 image_properties{};
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties{};
    PFN_vkGetPhysicalDeviceExternalSemaphoreProperties semaphore_properties{};
    PFN_vkCreateImage create_image{};
    PFN_vkDestroyImage destroy_image{};
    PFN_vkGetImageMemoryRequirements image_requirements{};
    PFN_vkGetMemoryWin32HandlePropertiesKHR handle_properties{};
    PFN_vkAllocateMemory allocate_memory{};
    PFN_vkFreeMemory free_memory{};
    PFN_vkBindImageMemory bind_image{};
    PFN_vkCreateSemaphore create_semaphore{};
    PFN_vkDestroySemaphore destroy_semaphore{};
    PFN_vkImportSemaphoreWin32HandleKHR import_semaphore{};
};

int32_t adapter_luid(const rsf_srvk_setup* setup, uint64_t* luid) {
    const auto get = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceProperties2"));
    if (!get) return VK_ERROR_EXTENSION_NOT_PRESENT;
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
    properties.pNext = &identity;
    get(static_cast<VkPhysicalDevice>(setup->physical_device), &properties);
    static_assert(sizeof(*luid) == VK_LUID_SIZE);
    std::memcpy(luid, identity.deviceLUID, sizeof(*luid));
    return identity.deviceLUIDValid && *luid ? VK_SUCCESS : VK_ERROR_INVALID_EXTERNAL_HANDLE;
}

int32_t open(const rsf_srvk_setup* setup, void** out) {
    auto* native = new (std::nothrow) Dispatch{};
    if (!native) return VK_ERROR_OUT_OF_HOST_MEMORY;
    native->physical = static_cast<VkPhysicalDevice>(setup->physical_device);
    native->device = static_cast<VkDevice>(setup->device);
#define INSTANCE(field, name) native->field = reinterpret_cast<PFN_##name>(setup->instance_proc(setup->instance, #name))
#define DEVICE(field, name) native->field = reinterpret_cast<PFN_##name>(setup->device_proc(setup->device, #name))
    INSTANCE(image_properties, vkGetPhysicalDeviceImageFormatProperties2);
    INSTANCE(memory_properties, vkGetPhysicalDeviceMemoryProperties);
    INSTANCE(semaphore_properties, vkGetPhysicalDeviceExternalSemaphoreProperties);
    DEVICE(create_image, vkCreateImage);
    DEVICE(destroy_image, vkDestroyImage);
    DEVICE(image_requirements, vkGetImageMemoryRequirements);
    DEVICE(handle_properties, vkGetMemoryWin32HandlePropertiesKHR);
    DEVICE(allocate_memory, vkAllocateMemory);
    DEVICE(free_memory, vkFreeMemory);
    DEVICE(bind_image, vkBindImageMemory);
    DEVICE(create_semaphore, vkCreateSemaphore);
    DEVICE(destroy_semaphore, vkDestroySemaphore);
    DEVICE(import_semaphore, vkImportSemaphoreWin32HandleKHR);
#undef INSTANCE
#undef DEVICE
    if (!native->image_properties || !native->memory_properties || !native->semaphore_properties ||
        !native->create_image || !native->destroy_image || !native->image_requirements ||
        !native->handle_properties || !native->allocate_memory || !native->free_memory ||
        !native->bind_image || !native->create_semaphore || !native->destroy_semaphore ||
        !native->import_semaphore) {
        delete native;
        return VK_ERROR_EXTENSION_NOT_PRESENT;
    }
    *out = native;
    return VK_SUCCESS;
}

void close(void* native) { delete static_cast<Dispatch*>(native); }

uint32_t timeline_importable(void* opaque) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkSemaphoreTypeCreateInfo type{VK_STRUCTURE_TYPE_SEMAPHORE_TYPE_CREATE_INFO};
    type.semaphoreType = VK_SEMAPHORE_TYPE_TIMELINE;
    VkPhysicalDeviceExternalSemaphoreInfo query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_SEMAPHORE_INFO};
    query.pNext = &type;
    query.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT;
    VkExternalSemaphoreProperties properties{VK_STRUCTURE_TYPE_EXTERNAL_SEMAPHORE_PROPERTIES};
    native.semaphore_properties(native.physical, &query, &properties);
    return (properties.externalSemaphoreFeatures & VK_EXTERNAL_SEMAPHORE_FEATURE_IMPORTABLE_BIT) != 0;
}

int32_t create_timeline(void* opaque, uint64_t* out) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkSemaphoreTypeCreateInfo type{VK_STRUCTURE_TYPE_SEMAPHORE_TYPE_CREATE_INFO};
    type.semaphoreType = VK_SEMAPHORE_TYPE_TIMELINE;
    VkSemaphoreCreateInfo create{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    create.pNext = &type;
    VkSemaphore timeline{};
    const auto result = native.create_semaphore(native.device, &create, nullptr, &timeline);
    *out = reinterpret_cast<uint64_t>(timeline);
    return result;
}

int32_t import_timeline(void* opaque, uint64_t timeline, void* handle) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkImportSemaphoreWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_SEMAPHORE_WIN32_HANDLE_INFO_KHR};
    import.semaphore = reinterpret_cast<VkSemaphore>(timeline);
    import.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT;
    import.handle = static_cast<HANDLE>(handle);
    return native.import_semaphore(native.device, &import);
}

void destroy_timeline(void* opaque, uint64_t timeline) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    native.destroy_semaphore(native.device, reinterpret_cast<VkSemaphore>(timeline), nullptr);
}

constexpr auto usage = VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;

int32_t image_properties(void* opaque, uint32_t format, uint32_t* importable, uint32_t* max_width, uint32_t* max_height) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkPhysicalDeviceExternalImageFormatInfo external_query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_IMAGE_FORMAT_INFO};
    external_query.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkPhysicalDeviceImageFormatInfo2 query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_IMAGE_FORMAT_INFO_2};
    query.pNext = &external_query;
    query.format = static_cast<VkFormat>(format);
    query.type = VK_IMAGE_TYPE_2D;
    query.tiling = VK_IMAGE_TILING_OPTIMAL;
    query.usage = usage;
    VkExternalImageFormatProperties external{VK_STRUCTURE_TYPE_EXTERNAL_IMAGE_FORMAT_PROPERTIES};
    VkImageFormatProperties2 properties{VK_STRUCTURE_TYPE_IMAGE_FORMAT_PROPERTIES_2};
    properties.pNext = &external;
    const auto result = native.image_properties(native.physical, &query, &properties);
    if (result == VK_SUCCESS) {
        *importable = (external.externalMemoryProperties.externalMemoryFeatures & VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT) != 0;
        *max_width = properties.imageFormatProperties.maxExtent.width;
        *max_height = properties.imageFormatProperties.maxExtent.height;
    }
    return result;
}

int32_t create_image(void* opaque, uint32_t format, uint32_t width, uint32_t height, uint64_t* out) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkExternalMemoryImageCreateInfo external{VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    external.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    create.pNext = &external;
    create.imageType = VK_IMAGE_TYPE_2D;
    create.format = static_cast<VkFormat>(format);
    create.extent = {width, height, 1};
    create.mipLevels = create.arrayLayers = 1;
    create.samples = VK_SAMPLE_COUNT_1_BIT;
    create.tiling = VK_IMAGE_TILING_OPTIMAL;
    create.usage = usage;
    VkImage image{};
    const auto result = native.create_image(native.device, &create, nullptr, &image);
    *out = reinterpret_cast<uint64_t>(image);
    return result;
}

void image_requirements(void* opaque, uint64_t image, uint64_t* size, uint32_t* types) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkMemoryRequirements requirements{};
    native.image_requirements(native.device, reinterpret_cast<VkImage>(image), &requirements);
    *size = requirements.size;
    *types = requirements.memoryTypeBits;
}

int32_t handle_memory_types(void* opaque, void* handle, uint32_t* types) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkMemoryWin32HandlePropertiesKHR properties{VK_STRUCTURE_TYPE_MEMORY_WIN32_HANDLE_PROPERTIES_KHR};
    const auto result = native.handle_properties(native.device,
        VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT, static_cast<HANDLE>(handle), &properties);
    if (result == VK_SUCCESS) *types = properties.memoryTypeBits;
    return result;
}

uint32_t device_local_types(void* opaque) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkPhysicalDeviceMemoryProperties memory{};
    native.memory_properties(native.physical, &memory);
    uint32_t types = 0;
    for (uint32_t i = 0; i < memory.memoryTypeCount && i < VK_MAX_MEMORY_TYPES; ++i)
        if (memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) types |= 1u << i;
    return types;
}

int32_t allocate_memory(void* opaque, uint64_t image, void* handle, uint64_t bytes, uint32_t type, uint64_t* out) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    VkMemoryDedicatedAllocateInfo dedicated{VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO};
    dedicated.image = reinterpret_cast<VkImage>(image);
    VkImportMemoryWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR};
    import.pNext = &dedicated;
    import.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    import.handle = static_cast<HANDLE>(handle);
    VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    allocate.pNext = &import;
    allocate.allocationSize = bytes;
    allocate.memoryTypeIndex = type;
    VkDeviceMemory memory{};
    const auto result = native.allocate_memory(native.device, &allocate, nullptr, &memory);
    *out = reinterpret_cast<uint64_t>(memory);
    return result;
}

int32_t bind_image(void* opaque, uint64_t image, uint64_t memory) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    return native.bind_image(native.device, reinterpret_cast<VkImage>(image), reinterpret_cast<VkDeviceMemory>(memory), 0);
}

void destroy_image(void* opaque, uint64_t image) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    native.destroy_image(native.device, reinterpret_cast<VkImage>(image), nullptr);
}

void free_memory(void* opaque, uint64_t memory) {
    const auto& native = *static_cast<const Dispatch*>(opaque);
    native.free_memory(native.device, reinterpret_cast<VkDeviceMemory>(memory), nullptr);
}

constexpr rsf_dx12vk_platform platform{
    adapter_luid, open, close, timeline_importable, create_timeline, import_timeline, destroy_timeline,
    image_properties, create_image, image_requirements, handle_memory_types, device_local_types,
    allocate_memory, bind_image, destroy_image, free_memory,
};
}

int32_t rsf_dx12vk_interop_create(const rsf_srvk_setup* setup, const rsf_dx12vk_interop_callbacks* callbacks,
    uint32_t timeline_enabled, rsf_dx12vk_interop** output) {
    return rsf_core_dx12vk_interop_create(&dx12vk_platform::platform, setup, callbacks, timeline_enabled, output);
}
