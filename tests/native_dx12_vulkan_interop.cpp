// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/orchestrator/src/native_dx12_vulkan_interop.cpp"
#include "../runtime/orchestrator/src/native_fg_xess_vulkan.cpp"
#include <cstdio>

static uint32_t luid_calls = 0;
extern "C" uint64_t RsfXessBridgeAdapterLuid(const rsf_xess_bridge*) { ++luid_calls; return 7; }
extern "C" int VulkanStoryXessFgCreateSharedImage(rsf_xess_bridge*, uint32_t, uint32_t, uint32_t, HANDLE*, ID3D12Resource**) { return -9; }
extern "C" void VulkanStoryXessFgReleaseImage(ID3D12Resource*) {}
extern "C" int VulkanStoryXessFgCreateSharedFence(rsf_xess_bridge*, HANDLE*) { return -9; }

namespace {
int failures = 0, images = 0, memories = 0, resources = 0, semaphores = 0;
int bridge_failure = 0;
bool import_failure = false, bind_failure = false;
HANDLE last_handle{};
#define CHECK(value) do { if (!(value)) { std::printf("line %d: %s\n", __LINE__, #value); ++failures; } } while (false)
void VKAPI_PTR properties(VkPhysicalDevice, VkPhysicalDeviceProperties2* out) {
    auto* identity = static_cast<VkPhysicalDeviceIDProperties*>(out->pNext);
    identity->deviceLUIDValid = VK_TRUE;
    const uint64_t luid = 7; std::memcpy(identity->deviceLUID, &luid, sizeof(luid));
}
VkResult VKAPI_PTR image_properties(VkPhysicalDevice, const VkPhysicalDeviceImageFormatInfo2* query, VkImageFormatProperties2* out) {
    CHECK(query->usage == (VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT));
    out->imageFormatProperties.maxExtent = {4096, 4096, 1};
    static_cast<VkExternalImageFormatProperties*>(out->pNext)->externalMemoryProperties.externalMemoryFeatures = VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT;
    return VK_SUCCESS;
}
void VKAPI_PTR memory_properties(VkPhysicalDevice, VkPhysicalDeviceMemoryProperties* out) {
    out->memoryTypeCount = 1; out->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
}
void VKAPI_PTR semaphore_properties(VkPhysicalDevice, const VkPhysicalDeviceExternalSemaphoreInfo* query, VkExternalSemaphoreProperties* out) {
    CHECK(query->handleType == VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT);
    out->externalSemaphoreFeatures = VK_EXTERNAL_SEMAPHORE_FEATURE_IMPORTABLE_BIT;
}
VkResult VKAPI_PTR create_image(VkDevice, const VkImageCreateInfo* info, const VkAllocationCallbacks*, VkImage* out) {
    CHECK(info->pNext && info->mipLevels == 1 && info->arrayLayers == 1);
    ++images; *out = reinterpret_cast<VkImage>(10); return VK_SUCCESS;
}
void VKAPI_PTR destroy_image(VkDevice, VkImage, const VkAllocationCallbacks*) { --images; }
void VKAPI_PTR requirements(VkDevice, VkImage, VkMemoryRequirements* out) { out->size = 4096; out->alignment = 256; out->memoryTypeBits = 1; }
VkResult VKAPI_PTR handle_properties(VkDevice, VkExternalMemoryHandleTypeFlagBits type, HANDLE handle, VkMemoryWin32HandlePropertiesKHR* out) {
    CHECK(type == VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT && handle == last_handle);
    out->memoryTypeBits = 1; return VK_SUCCESS;
}
VkResult VKAPI_PTR allocate(VkDevice, const VkMemoryAllocateInfo* info, const VkAllocationCallbacks*, VkDeviceMemory* out) {
    auto* import = static_cast<const VkImportMemoryWin32HandleInfoKHR*>(info->pNext);
    CHECK(import && import->handle == last_handle && import->pNext && info->allocationSize == 4096 && info->memoryTypeIndex == 0);
    ++memories; *out = reinterpret_cast<VkDeviceMemory>(20); return VK_SUCCESS;
}
void VKAPI_PTR free_memory(VkDevice, VkDeviceMemory, const VkAllocationCallbacks*) { --memories; }
VkResult VKAPI_PTR bind(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) { return bind_failure ? VK_ERROR_UNKNOWN : VK_SUCCESS; }
VkResult VKAPI_PTR create_semaphore(VkDevice, const VkSemaphoreCreateInfo* info, const VkAllocationCallbacks*, VkSemaphore* out) {
    CHECK(static_cast<const VkSemaphoreTypeCreateInfo*>(info->pNext)->semaphoreType == VK_SEMAPHORE_TYPE_TIMELINE);
    ++semaphores; *out = reinterpret_cast<VkSemaphore>(30); return VK_SUCCESS;
}
void VKAPI_PTR destroy_semaphore(VkDevice, VkSemaphore, const VkAllocationCallbacks*) { --semaphores; }
VkResult VKAPI_PTR import_semaphore(VkDevice, const VkImportSemaphoreWin32HandleInfoKHR* info) {
    CHECK(info->handle == last_handle && info->flags == 0);
    return import_failure ? VK_ERROR_INVALID_EXTERNAL_HANDLE : VK_SUCCESS;
}
rsf_vk_function proc(void*, const char* name) {
#define ENTRY(symbol, function) if (std::strcmp(name, #symbol) == 0) return reinterpret_cast<rsf_vk_function>(function)
    ENTRY(vkGetPhysicalDeviceProperties2, properties);
    ENTRY(vkGetPhysicalDeviceImageFormatProperties2, image_properties);
    ENTRY(vkGetPhysicalDeviceMemoryProperties, memory_properties);
    ENTRY(vkGetPhysicalDeviceExternalSemaphoreProperties, semaphore_properties);
    ENTRY(vkCreateImage, create_image); ENTRY(vkDestroyImage, destroy_image); ENTRY(vkGetImageMemoryRequirements, requirements);
    ENTRY(vkGetMemoryWin32HandlePropertiesKHR, handle_properties); ENTRY(vkAllocateMemory, allocate); ENTRY(vkFreeMemory, free_memory);
    ENTRY(vkBindImageMemory, bind); ENTRY(vkCreateSemaphore, create_semaphore); ENTRY(vkDestroySemaphore, destroy_semaphore);
    ENTRY(vkImportSemaphoreWin32HandleKHR, import_semaphore);
#undef ENTRY
    return nullptr;
}
int32_t shared_image(void*, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource) {
    CHECK(width == 32 && height == 24 && format == VK_FORMAT_R32_SFLOAT);
    *handle = last_handle = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    *resource = new int(1); ++resources; return bridge_failure;
}
void release_image(void* resource) { delete static_cast<int*>(resource); --resources; }
int32_t shared_fence(void*, void** handle) { *handle = last_handle = CreateEventW(nullptr, FALSE, FALSE, nullptr); return bridge_failure; }
void closed() { DWORD flags = 0; CHECK(!GetHandleInformation(last_handle, &flags) && GetLastError() == ERROR_INVALID_HANDLE); }
}
int main() {
    rsf_srvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
    setup.instance = setup.physical_device = setup.device = reinterpret_cast<void*>(1);
    setup.instance_proc = setup.device_proc = proc;
    auto* bridge = reinterpret_cast<rsf_xess_bridge*>(1);
    rsf_xessvk_interop* xess = reinterpret_cast<rsf_xessvk_interop*>(1);
    CHECK(rsf_xessvk_interop_create(nullptr, bridge, 1, &xess) == RSF_XESS_BRIDGE_INVALID_ARGUMENT && !xess && !luid_calls);
    setup.struct_size--;
    CHECK(rsf_xessvk_interop_create(&setup, bridge, 1, &xess) == RSF_XESS_BRIDGE_INVALID_ARGUMENT && !xess && !luid_calls);
    setup.struct_size++; setup.abi_version++;
    CHECK(rsf_xessvk_interop_create(&setup, bridge, 1, &xess) == RSF_XESS_BRIDGE_ABI_MISMATCH && !xess && !luid_calls);
    setup.abi_version = RSF_SRVK_ABI_VERSION;
    CHECK(rsf_xessvk_interop_create(&setup, bridge, 1, &xess) == -9 && !xess && luid_calls == 1 && !semaphores);
    rsf_dx12vk_interop_callbacks callbacks{sizeof(callbacks), 1, 7, nullptr, shared_image, release_image, shared_fence};
    rsf_dx12vk_interop* owner = reinterpret_cast<rsf_dx12vk_interop*>(1);
    callbacks.struct_size--;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 1, &owner) == RSF_DX12VK_INTEROP_INVALID_ARGUMENT && !owner);
    callbacks.struct_size++; callbacks.abi_version++;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 1, &owner) == RSF_DX12VK_INTEROP_ABI_MISMATCH && !owner);
    callbacks.abi_version = 1; callbacks.adapter_luid = 8;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 1, &owner) == VK_ERROR_INVALID_EXTERNAL_HANDLE && !owner && !semaphores);
    callbacks.adapter_luid = 7;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 0, &owner) == RSF_DX12VK_INTEROP_INVALID_ARGUMENT && !owner);
    bridge_failure = -77;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 1, &owner) == -77 && !owner && !semaphores); closed();
    bridge_failure = 0; import_failure = true;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 1, &owner) == VK_ERROR_INVALID_EXTERNAL_HANDLE && !owner && !semaphores); closed();
    import_failure = false;
    CHECK(rsf_dx12vk_interop_create(&setup, &callbacks, 1, &owner) == 0 && owner && semaphores == 1); closed();
    CHECK(rsf_dx12vk_interop_timeline(owner) == 30);
    rsf_dx12vk_shared_image image{};
    bridge_failure = -78;
    CHECK(rsf_dx12vk_interop_image(owner, VK_FORMAT_R32_SFLOAT, 32, 24, &image) == -78 && !image.image && !resources); closed();
    bridge_failure = 0; bind_failure = true;
    CHECK(rsf_dx12vk_interop_image(owner, VK_FORMAT_R32_SFLOAT, 32, 24, &image) == VK_ERROR_UNKNOWN && !image.image);
    CHECK(!images && !memories && !resources); closed();
    bind_failure = false;
    CHECK(rsf_dx12vk_interop_image(owner, VK_FORMAT_R32_SFLOAT, 32, 24, &image) == 0);
    CHECK(image.image == 10 && image.dx12_resource && image.width == 32 && image.height == 24); closed();
    CHECK(rsf_dx12vk_interop_destroy(owner, 0) == RSF_DX12VK_INTEROP_INVALID_ARGUMENT);
    CHECK(images == 1 && memories == 1 && resources == 1 && semaphores == 1);
    CHECK(rsf_dx12vk_interop_destroy(owner, 1) == 0);
    CHECK(!images && !memories && !resources && !semaphores);
    if (!failures) std::puts("Common DX12/Vulkan import ownership and failure cleanup passed (synthetic, no GPU).");
    return failures ? 1 : 0;
}
