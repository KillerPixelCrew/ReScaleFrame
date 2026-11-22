// SPDX-License-Identifier: MIT
// The Unity package's Vulkan bridge: Unity's Vulkan images are copied into same-adapter D3D12
// resources for the render pass, and Unity's present is replaced by this bridge's DXGI swap chain.
// Shared device readiness, the fault, queued render events and their packets, present and surface
// entries, the window frame identity, the DXGI swap chain and its resize, and the checked release
// are Rust (vulkan_state.rs), as are the intercepted swapchains with their images and present-ready
// semaphores, the presentation window's identity and the presentation lease. The command slot
// rotation and shared fence values come from this route's Runtime submission handle.
// Rust owns the shared device, queue, fence and imported semaphore, as well as the pool, slots
// and shared images. Unity's Vulkan resolver keys and routes, the registered interceptors, the
// saved loader functions and the interop device-extension selection are vulkan_resolver.rs.
// This file keeps Unity interface access and typed graphics, loader and Win32 calls.
#include "vulkan_bridge.h"
#include "vulkan_state.h"
#include "bridge_interop.h"
#include "bridge_state.h"
#include "package.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsVulkan.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <array>
#include <cstddef>
#include <cstring>
#include <vector>

// Only the Vulkan commands still implemented in this translation unit cross into the resolver.
struct VulkanHooks {
    PFN_vkQueuePresentKHR present;
    PFN_vkGetDeviceProcAddr device_proc;
    PFN_vkEnumerateDeviceExtensionProperties enumerate;
    PFN_vkCreateDevice create_device;
    PFN_vkCreateSemaphore create_semaphore;
    PFN_vkDestroySemaphore destroy_semaphore;
    PFN_vkDeviceWaitIdle wait_idle;
    void (*retirement_fault)();
};
extern "C" PFN_vkGetInstanceProcAddr rsf_unity_vk_install(PFN_vkGetInstanceProcAddr, const VulkanHooks*);
extern "C" bool rsf_unity_vk_interop_ready(VkDevice, PFN_vkGetDeviceProcAddr);
static_assert(sizeof(VulkanHooks) == 8 * sizeof(void*));
static_assert(sizeof(VkExtensionProperties) == 260 && offsetof(VkExtensionProperties, specVersion) == 256);
static_assert(sizeof(VkSwapchainCreateInfoKHR) == 104 && offsetof(VkSwapchainCreateInfoKHR, imageUsage) == 56 &&
    offsetof(VkSwapchainCreateInfoKHR, oldSwapchain) == 96);
static_assert(sizeof(VkWin32SurfaceCreateInfoKHR) == 40 && offsetof(VkWin32SurfaceCreateInfoKHR, hwnd) == 32);
static_assert(sizeof(VkSemaphoreCreateInfo) == 24);
static_assert(sizeof(VkDeviceCreateInfo) == 72 && offsetof(VkDeviceCreateInfo, enabledExtensionCount) == 48 &&
    offsetof(VkDeviceCreateInfo, ppEnabledExtensionNames) == 56);

// Private package receipt ABI shared with the D3D11 and D3D12 producers.
extern "C" uint64_t rsf_unity_package_sr_ring_begin(void);
extern "C" void rsf_unity_package_sr_ring_retired(uint64_t epoch);
extern "C" void rsf_unity_package_sr_recording(void* commands, uint64_t epoch, uint32_t slot,
    uint64_t recording, uint64_t retired_through);

static_assert(sizeof(rsf_unity_vulkan_present_source) == 24, "vulkan_state.rs PresentSource");
static_assert(sizeof(rsf_unity_vulkan_image_handles) == 24);
static_assert(sizeof(rsf_unity_vulkan_device_handles) == 64);
static_assert(offsetof(rsf_unity_vulkan_device_handles, vk_device) == 24);
static_assert(offsetof(rsf_unity_vulkan_device_handles, vk_queue_family) == 56);
static_assert(sizeof(rsf_unity_vulkan_device_ops) == 14 * sizeof(void*));
static_assert(sizeof(rsf_unity_vulkan_surface_ops) == 5 * sizeof(void*));
static_assert(sizeof(rsf_unity_vulkan_command_handles) == 40);
static_assert(offsetof(rsf_unity_vulkan_command_handles, commands) == 8);
static_assert(offsetof(rsf_unity_vulkan_command_handles, vk) == 16);
static_assert(offsetof(rsf_unity_vulkan_command_handles, complete) == 32);
static_assert(sizeof(rsf_unity_vulkan_frame) == sizeof(rsf_unity_packet) + sizeof(rsf_game_host_services));
static_assert(offsetof(rsf_unity_vulkan_frame, services) == sizeof(rsf_unity_packet));

namespace {
using Microsoft::WRL::ComPtr;
IUnityGraphicsVulkanV2* unity = nullptr;
int base_event = 0;
// Vulkan handles cross to vulkan_state.rs as 64-bit identity.
template <class Handle> uint64_t handle_id(Handle handle) { return reinterpret_cast<uint64_t>(handle); }
template <class Handle> Handle from_id(uint64_t id) { return reinterpret_cast<Handle>(id); }
// A borrowed view of the Rust owner, valid during creation, admission or checked release.
struct DeviceView {
    rsf_unity_vulkan_device_handles handles{};
    ID3D12Device* device() const { return static_cast<ID3D12Device*>(handles.device); }
    ID3D12CommandQueue* queue() const { return static_cast<ID3D12CommandQueue*>(handles.queue); }
    ID3D12Fence* shared_fence() const { return static_cast<ID3D12Fence*>(handles.fence); }
    VkDevice vk_device() const { return from_id<VkDevice>(handles.vk_device); }
    VkPhysicalDevice physical_device() const { return from_id<VkPhysicalDevice>(handles.vk_physical_device); }
    VkQueue vk_queue() const { return from_id<VkQueue>(handles.vk_queue); }
    VkSemaphore semaphore() const { return from_id<VkSemaphore>(handles.vk_semaphore); }
    uint32_t queue_family() const { return handles.vk_queue_family; }
};
// Only a view. Rust retains the imported image and D3D12 resource for the GPU lifetime.
struct ImageView {
    rsf_unity_vulkan_image_handles handles{};
    VkImage image() const { return reinterpret_cast<VkImage>(handles.image); }
    ID3D12Resource* resource() const { return static_cast<ID3D12Resource*>(handles.resource); }
};
struct SlotView {
    rsf_unity_vulkan_command_handles handles{};
    ID3D12CommandAllocator* allocator() const { return static_cast<ID3D12CommandAllocator*>(handles.allocator); }
    ID3D12GraphicsCommandList* commands() const { return static_cast<ID3D12GraphicsCommandList*>(handles.commands); }
    VkCommandBuffer vk(std::size_t index) const { return from_id<VkCommandBuffer>(handles.vk[index]); }
    VkFence complete() const { return from_id<VkFence>(handles.complete); }
};
// Unity's images for each queued render event, indexed by its event slot (vulkan_state.h).
std::array<std::array<UnityVulkanImage, 4>, RSF_UNITY_FRAMES_PACKETS> event_images{};
HWND surface_window() { return static_cast<HWND>(rsf_unity_vulkan_surface_window()); }
// Ends the presentation lease (vulkan_state.h) the caller took.
struct PresentationLease {
    bool held = true;
    PresentationLease() = default;
    explicit PresentationLease(bool taken) : held(taken) {}
    PresentationLease(const PresentationLease&) = delete;
    PresentationLease& operator=(const PresentationLease&) = delete;
    ~PresentationLease() { if (held) rsf_unity_vulkan_presentation_end(); }
};
void log(const rsf_game_host_services* services, const char* text)
{
    if (services && services->log) services->log(services->user, text);
    else { OutputDebugStringA(text); OutputDebugStringA("\n"); }
}
// Stops the bridge until release. The first fault is reported, so the package status and the
// managed jitter gate see it instead of a silent stop.
void fault(int32_t result, const char* message)
{
    if (rsf_unity_vulkan_fault()) rsf_unity_package_bridge_error(result, message);
}
void stop(const char* message) { fault(RSF_BACKEND_ERROR_FEATURE_FAILED, message); }
// A present interception or surface update in progress. Keep admission through the forwarded
// Present so release cannot drain the Vulkan queue during that call.
struct BridgeEntry {
    bool admitted = rsf_unity_vulkan_enter() != 0;
    BridgeEntry() = default;
    BridgeEntry(const BridgeEntry&) = delete;
    BridgeEntry& operator=(const BridgeEntry&) = delete;
    ~BridgeEntry() { if (admitted) rsf_unity_vulkan_leave(); }
};
void dx_barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}
uint64_t shared_completed(const rsf_unity_vulkan_device_handles* shared) noexcept
{
    return DeviceView{*shared}.shared_fence()->GetCompletedValue();
}
bool drain(const DeviceView& graphics)
{
    if (vkQueueWaitIdle(graphics.vk_queue()) != VK_SUCCESS) return false;
    const uint64_t completed = shared_completed(&graphics.handles);
    const auto* ring = rsf_unity_vulkan_submission_ring();
    const auto status = rsf_submission_status(ring, completed);
    if (status == RSF_SUBMISSION_REMOVED) return false;
    if (status == RSF_SUBMISSION_PENDING) {
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
        const bool done = SUCCEEDED(graphics.shared_fence()->SetEventOnCompletion(rsf_submission_pending_value(ring), event)) &&
            WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
        CloseHandle(event);
        const uint64_t after_wait = shared_completed(&graphics.handles);
        return done && rsf_submission_status(ring, after_wait) == RSF_SUBMISSION_COMPLETE;
    }
    return true;
}
void destroy_image(const rsf_unity_vulkan_device_handles* shared, const rsf_unity_vulkan_image_handles* image) noexcept
{
    const DeviceView graphics{*shared};
    if (image->image) vkDestroyImage(graphics.vk_device(), from_id<VkImage>(image->image), nullptr);
    if (image->memory) vkFreeMemory(graphics.vk_device(), from_id<VkDeviceMemory>(image->memory), nullptr);
    if (image->resource) static_cast<ID3D12Resource*>(image->resource)->Release();
}
// Called after command allocation rolled back or after Rust retired the images and commands.
void destroy_shared(const rsf_unity_vulkan_device_handles* shared) noexcept
{
    const DeviceView graphics{*shared};
    if (const auto semaphore = graphics.semaphore()) vkDestroySemaphore(graphics.vk_device(), semaphore, nullptr);
    if (auto* fence = graphics.shared_fence()) fence->Release();
    if (auto* queue = graphics.queue()) queue->Release();
    if (auto* device = graphics.device()) device->Release();
}
bool create_shared_image(const rsf_unity_vulkan_device_handles* shared, uint32_t width, uint32_t height, uint32_t format_value,
    uint32_t dx_format_value, rsf_unity_vulkan_image_handles* output) noexcept
{
    const auto format = static_cast<VkFormat>(format_value);
    const auto dx_format = static_cast<DXGI_FORMAT>(dx_format_value);
    const DeviceView graphics{*shared};
    ComPtr<ID3D12Resource> resource;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC description{}; description.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    description.Width = width; description.Height = height; description.DepthOrArraySize = description.MipLevels = 1;
    description.Format = dx_format; description.SampleDesc.Count = 1;
    description.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    if (FAILED(graphics.device()->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &description, D3D12_RESOURCE_STATE_COMMON,
        nullptr, IID_PPV_ARGS(&resource)))) return false;
    auto* shared_resource = resource.Detach();
    output->resource = shared_resource;
    VkExternalMemoryImageCreateInfo external{VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    external.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkImageCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}; info.pNext = &external;
    info.imageType = VK_IMAGE_TYPE_2D; info.format = format; info.extent = {width, height, 1};
    info.mipLevels = info.arrayLayers = 1; info.samples = VK_SAMPLE_COUNT_1_BIT; info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    VkImage image = VK_NULL_HANDLE;
    if (vkCreateImage(graphics.vk_device(), &info, nullptr, &image) != VK_SUCCESS) return false;
    output->image = handle_id(image);
    VkMemoryRequirements requirements{}; vkGetImageMemoryRequirements(graphics.vk_device(), image, &requirements);
    VkPhysicalDeviceMemoryProperties properties{}; vkGetPhysicalDeviceMemoryProperties(graphics.physical_device(), &properties);
    uint32_t memory_type = UINT32_MAX;
    for (uint32_t i = 0; i < properties.memoryTypeCount; ++i)
        if ((requirements.memoryTypeBits & (1u << i)) && (properties.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { memory_type = i; break; }
    HANDLE handle = nullptr;
    if (memory_type == UINT32_MAX || FAILED(graphics.device()->CreateSharedHandle(shared_resource, nullptr, GENERIC_ALL, nullptr, &handle))) return false;
    VkMemoryDedicatedAllocateInfo dedicated{VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO}; dedicated.image = image;
    VkImportMemoryWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR};
    import.pNext = &dedicated; import.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT; import.handle = handle;
    VkMemoryAllocateInfo allocation{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocation.pNext = &import;
    allocation.allocationSize = requirements.size; allocation.memoryTypeIndex = memory_type;
    VkDeviceMemory allocated_memory = VK_NULL_HANDLE;
    const auto allocated = vkAllocateMemory(graphics.vk_device(), &allocation, nullptr, &allocated_memory);
    CloseHandle(handle);
    if (allocated != VK_SUCCESS) return false;
    output->memory = handle_id(allocated_memory);
    return vkBindImageMemory(graphics.vk_device(), image, allocated_memory, 0) == VK_SUCCESS;
}

bool create_image(uint32_t index, uint32_t width, uint32_t height, VkFormat format,
    DXGI_FORMAT dx_format, ImageView& view)
{
    return rsf_unity_vulkan_prepare_image(index, width, height, static_cast<uint32_t>(format),
        static_cast<uint32_t>(dx_format), &view.handles);
}
uint64_t create_command_pool(const rsf_unity_vulkan_device_handles* shared) noexcept
{
    const DeviceView graphics{*shared};
    VkCommandPoolCreateInfo info{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
    info.queueFamilyIndex = graphics.queue_family();
    info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    VkCommandPool pool = VK_NULL_HANDLE;
    return vkCreateCommandPool(graphics.vk_device(), &info, nullptr, &pool) == VK_SUCCESS ? handle_id(pool) : 0;
}
void destroy_command_pool(const rsf_unity_vulkan_device_handles* shared, uint64_t pool) noexcept
{
    vkDestroyCommandPool(DeviceView{*shared}.vk_device(), from_id<VkCommandPool>(pool), nullptr);
}
bool create_command_slot(const rsf_unity_vulkan_device_handles* shared, uint64_t pool, rsf_unity_vulkan_command_handles* slot) noexcept
{
    const DeviceView graphics{*shared};
    VkCommandBufferAllocateInfo allocate{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
    allocate.commandPool = from_id<VkCommandPool>(pool);
    allocate.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocate.commandBufferCount = 2;
    VkCommandBuffer buffers[2]{};
    if (vkAllocateCommandBuffers(graphics.vk_device(), &allocate, buffers) != VK_SUCCESS) return false;
    slot->vk[0] = handle_id(buffers[0]); slot->vk[1] = handle_id(buffers[1]);

    VkFenceCreateInfo info{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO}; info.flags = VK_FENCE_CREATE_SIGNALED_BIT;
    VkFence complete = VK_NULL_HANDLE;
    if (vkCreateFence(graphics.vk_device(), &info, nullptr, &complete) != VK_SUCCESS) return false;
    slot->complete = handle_id(complete);

    ComPtr<ID3D12CommandAllocator> allocator;
    if (FAILED(graphics.device()->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)))) return false;
    auto* commands_allocator = allocator.Detach();
    slot->allocator = commands_allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    if (FAILED(graphics.device()->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
        commands_allocator, nullptr, IID_PPV_ARGS(&commands)))) return false;
    auto* list = commands.Detach();
    slot->commands = list;
    return SUCCEEDED(list->Close());
}
void destroy_command_slot(const rsf_unity_vulkan_device_handles* shared, const rsf_unity_vulkan_command_handles* slot) noexcept
{
    if (slot->complete) vkDestroyFence(DeviceView{*shared}.vk_device(), from_id<VkFence>(slot->complete), nullptr);
    if (slot->commands) static_cast<ID3D12GraphicsCommandList*>(slot->commands)->Release();
    if (slot->allocator) static_cast<ID3D12CommandAllocator*>(slot->allocator)->Release();
}
bool create_shared(const rsf_game_host_services* services, rsf_unity_vulkan_device_handles* output) noexcept try
{
    const auto instance = unity->Instance();
    if (!instance.device || !instance.physicalDevice || !instance.graphicsQueue) return false;
    output->vk_device = handle_id(instance.device);
    output->vk_physical_device = handle_id(instance.physicalDevice);
    output->vk_queue = handle_id(instance.graphicsQueue);
    output->vk_queue_family = instance.queueFamilyIndex;
    if (!rsf_unity_vk_interop_ready(instance.device, vkGetDeviceProcAddr)) {
        log(services, "Unity Vulkan device lacks enabled Win32 external-memory/semaphore extensions.");
        return false;
    }
    VkPhysicalDeviceIDProperties id{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; properties.pNext = &id;
    vkGetPhysicalDeviceProperties2(instance.physicalDevice, &properties);
    if (!id.deviceLUIDValid) { log(services, "Unity Vulkan adapter supplied no Windows LUID."); return false; }
    LUID luid{}; std::memcpy(&luid, id.deviceLUID, sizeof(luid));
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
    ComPtr<ID3D12Device> device;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&adapter))) ||
        FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) return false;
    auto* shared_device = device.Detach();
    output->device = shared_device;
    D3D12_COMMAND_QUEUE_DESC q{};
    ComPtr<ID3D12CommandQueue> queue;
    if (FAILED(shared_device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)))) return false;
    output->queue = queue.Detach();
    ComPtr<ID3D12Fence> fence;
    if (FAILED(shared_device->CreateFence(0, D3D12_FENCE_FLAG_SHARED, IID_PPV_ARGS(&fence)))) return false;
    auto* shared_fence = fence.Detach();
    output->fence = shared_fence;
    HANDLE handle = nullptr;
    if (FAILED(shared_device->CreateSharedHandle(shared_fence, nullptr, GENERIC_ALL, nullptr, &handle))) return false;
    VkSemaphoreCreateInfo semaphore_info{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    VkSemaphore created = VK_NULL_HANDLE;
    auto result = vkCreateSemaphore(instance.device, &semaphore_info, nullptr, &created);
    if (result == VK_SUCCESS) {
        output->vk_semaphore = handle_id(created);
        const auto import_semaphore = reinterpret_cast<PFN_vkImportSemaphoreWin32HandleKHR>(vkGetDeviceProcAddr(instance.device, "vkImportSemaphoreWin32HandleKHR"));
        VkImportSemaphoreWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_SEMAPHORE_WIN32_HANDLE_INFO_KHR};
        import.semaphore = created; import.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT; import.handle = handle;
        result = import_semaphore ? import_semaphore(instance.device, &import) : VK_ERROR_EXTENSION_NOT_PRESENT;
    }
    CloseHandle(handle);
    return result == VK_SUCCESS;
} catch (...) { return false; }
bool command_fence_ready(const rsf_unity_vulkan_device_handles* shared, uint64_t fence) noexcept
{
    return vkGetFenceStatus(DeviceView{*shared}.vk_device(), from_id<VkFence>(fence)) == VK_SUCCESS;
}
void report_created(const rsf_game_host_services* services) noexcept try
{
    log(services, "Unity Vulkan textures share same-adapter D3D12 resources and GPU fences.");
} catch (...) {}
constexpr rsf_unity_vulkan_device_ops device_ops{
    create_shared, destroy_shared, create_command_pool, destroy_command_pool,
    create_command_slot, destroy_command_slot, create_shared_image, destroy_image,
    command_fence_ready, shared_completed,
    [](const rsf_unity_vulkan_device_handles* shared) noexcept { return drain(DeviceView{*shared}); },
    report_created, rsf_unity_package_sr_ring_begin, rsf_unity_package_sr_ring_retired,
};
uint32_t initialize(DeviceView& graphics, const rsf_game_host_services* services = nullptr)
{
    return unity ? rsf_unity_vulkan_initialize(services, &device_ops, &graphics.handles) : RSF_UNITY_VULKAN_DEVICE_FAILED;
}
bool begin(const DeviceView& graphics, const SlotView& slot, uint32_t ring)
{
    const VkFence complete = slot.complete();
    if (vkWaitForFences(graphics.vk_device(), 1, &complete, VK_TRUE, 10000000000ull) != VK_SUCCESS) return false;
    for (const uint64_t buffer : slot.handles.vk) {
        const auto command = from_id<VkCommandBuffer>(buffer);
        if (vkResetCommandBuffer(command, 0) != VK_SUCCESS) return false;
        VkCommandBufferBeginInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        if (vkBeginCommandBuffer(command, &info) != VK_SUCCESS) return false;
    }
    if (FAILED(slot.allocator()->Reset()) || FAILED(slot.commands()->Reset(slot.allocator(), nullptr)))
        return false;
    uint64_t epoch = 0;
    const auto serial = rsf_unity_vulkan_begin_recording(ring, &epoch);
    if (!epoch || !serial) return false;
    rsf_unity_package_sr_recording(slot.commands(), epoch, ring, serial, serial - 1);
    return true;
}
void image_barrier(VkCommandBuffer commands, VkImage image, VkImageLayout before, VkImageLayout after,
    uint32_t from, uint32_t to, VkAccessFlags source, VkAccessFlags target)
{
    VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER}; barrier.srcAccessMask = source; barrier.dstAccessMask = target;
    barrier.oldLayout = before; barrier.newLayout = after; barrier.srcQueueFamilyIndex = from; barrier.dstQueueFamilyIndex = to;
    barrier.image = image; barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    vkCmdPipelineBarrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}
void acquire_image(const DeviceView& graphics, VkCommandBuffer commands, uint32_t index, const ImageView& image, bool output)
{
    const auto old_layout = rsf_unity_vulkan_record_image(index) ?
        VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED;
    image_barrier(commands, image.image(), old_layout,
        output ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL : VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_QUEUE_FAMILY_EXTERNAL, graphics.queue_family(), 0, output ? VK_ACCESS_TRANSFER_READ_BIT : VK_ACCESS_TRANSFER_WRITE_BIT);
}
void release_image(const DeviceView& graphics, VkCommandBuffer commands, const ImageView& image, bool output)
{
    image_barrier(commands, image.image(), output ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL : VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_GENERAL, graphics.queue_family(), VK_QUEUE_FAMILY_EXTERNAL,
        output ? VK_ACCESS_TRANSFER_READ_BIT : VK_ACCESS_TRANSFER_WRITE_BIT, 0);
}
// Vulkan signals the next shared fence value after the copies; the D3D12 queue waits for it.
bool upload(const DeviceView& graphics, const SlotView& slot, const VkSemaphore* waits = nullptr, uint32_t wait_count = 0)
{
    const auto command = slot.vk(0);
    const auto semaphore = graphics.semaphore();
    if (vkEndCommandBuffer(command) != VK_SUCCESS) return false;
    std::vector<VkPipelineStageFlags> stages(wait_count, VK_PIPELINE_STAGE_TRANSFER_BIT);
    std::vector<uint64_t> values(wait_count, 0);
    const uint64_t value = rsf_submission_next_completion(rsf_unity_vulkan_submission_ring());
    if (!value) return false;
    VkD3D12FenceSubmitInfoKHR fence{VK_STRUCTURE_TYPE_D3D12_FENCE_SUBMIT_INFO_KHR};
    fence.waitSemaphoreValuesCount = wait_count; fence.pWaitSemaphoreValues = values.data();
    fence.signalSemaphoreValuesCount = 1; fence.pSignalSemaphoreValues = &value;
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.pNext = &fence;
    submit.waitSemaphoreCount = wait_count; submit.pWaitSemaphores = waits; submit.pWaitDstStageMask = stages.data();
    submit.commandBufferCount = 1; submit.pCommandBuffers = &command; submit.signalSemaphoreCount = 1; submit.pSignalSemaphores = &semaphore;
    if (vkQueueSubmit(graphics.vk_queue(), 1, &submit, VK_NULL_HANDLE) != VK_SUCCESS) return false;
    rsf_unity_vulkan_upload_submitted();
    if (rsf_submission_completion(rsf_unity_vulkan_submission_ring()) != value) return false;
    return SUCCEEDED(graphics.queue()->Wait(graphics.shared_fence(), value));
}
// The D3D12 queue signals the ring slot's shared fence value; Vulkan waits for it before the
// hand-back copies and signals the slot's fence.
bool submit(const DeviceView& graphics, const SlotView& slot, uint32_t ring, VkSemaphore ready = VK_NULL_HANDLE)
{
    const auto command = slot.vk(1);
    const auto semaphore = graphics.semaphore();
    if (FAILED(slot.commands()->Close()) || vkEndCommandBuffer(command) != VK_SUCCESS) return false;
    const uint64_t value = rsf_submission_submit(rsf_unity_vulkan_submission_ring(), ring);
    if (!value) return false;
    ID3D12CommandList* lists[] = {slot.commands()}; graphics.queue()->ExecuteCommandLists(1, lists);
    if (FAILED(graphics.queue()->Signal(graphics.shared_fence(), value))) return false;
    VkD3D12FenceSubmitInfoKHR fence{VK_STRUCTURE_TYPE_D3D12_FENCE_SUBMIT_INFO_KHR};
    fence.waitSemaphoreValuesCount = 1; fence.pWaitSemaphoreValues = &value;
    uint64_t zero = 0; fence.signalSemaphoreValuesCount = ready ? 1u : 0u; fence.pSignalSemaphoreValues = &zero;
    VkPipelineStageFlags stage = VK_PIPELINE_STAGE_TRANSFER_BIT;
    VkSubmitInfo submit_info{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit_info.pNext = &fence;
    submit_info.waitSemaphoreCount = 1; submit_info.pWaitSemaphores = &semaphore; submit_info.pWaitDstStageMask = &stage;
    submit_info.commandBufferCount = 1; submit_info.pCommandBuffers = &command;
    submit_info.signalSemaphoreCount = ready ? 1u : 0u; submit_info.pSignalSemaphores = &ready;
    const VkFence complete = slot.complete();
    if (vkResetFences(graphics.vk_device(), 1, &complete) != VK_SUCCESS) return false;
    if (vkQueueSubmit(graphics.vk_queue(), 1, &submit_info, complete) != VK_SUCCESS) return false;
    rsf_unity_vulkan_handback_fenced();
    return true;
}
void copy(VkCommandBuffer commands, VkImage source, VkImageLayout source_layout, VkImageAspectFlags aspect,
    VkImage target, VkImageLayout target_layout, uint32_t width, uint32_t height)
{
    VkImageCopy region{}; region.srcSubresource = {aspect, 0, 0, 1}; region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {width, height, 1}; vkCmdCopyImage(commands, source, source_layout, target, target_layout, 1, &region);
}
void execute_admitted(const rsf_unity_vulkan_frame& frame, uint32_t kind, uint32_t reconfigure, uint32_t index) noexcept try
{
    PresentationLease lease{rsf_unity_vulkan_presentation_try_begin() != 0};
    if (!lease.held) return;
    if (index >= event_images.size() || !frame.services.render_pass) return;
    const auto& host = frame.services;
    const auto& packet = frame.packet;
    const auto& textures = event_images[index];
    std::array<ImageView, 6> images{};
    const bool present = kind == RSF_UNITY_VULKAN_EVENT_WINDOW, hudless = kind == RSF_UNITY_VULKAN_EVENT_HUDLESS;
    DeviceView graphics;
    const uint32_t ready = initialize(graphics, &host);
    if (ready == RSF_UNITY_VULKAN_DEVICE_BUSY) return;
    if (ready != RSF_UNITY_VULKAN_DEVICE_READY) {
        fault(RSF_BACKEND_ERROR_INIT_FAILED, "Unity Vulkan shared device/fence initialization failed; native output remains in place.");
        return;
    }
    const uint32_t ring = rsf_submission_acquire(rsf_unity_vulkan_submission_ring(), nullptr);
    SlotView slot;
    if (!rsf_unity_vulkan_command_slot(ring, &slot.handles) || !begin(graphics, slot, ring)) {
        stop("Unity Vulkan command slot could not be reused; the bridge stopped.");
        return;
    }
    if (reconfigure && !drain(graphics)) { stop("Unity Vulkan GPU drain before reconfiguration failed; the bridge stopped."); return; }
    constexpr VkFormat formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
    constexpr DXGI_FORMAT dx_formats[] = {DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT};
    const bool probe = (packet.flags & RSF_UNITY_PACKET_PROBE) != 0;
    if (hudless || present) {
        const auto& texture = textures[0];
        const uint32_t target = present ? 5u : 4u;
        if (!create_image(target, packet.camera.output_width, packet.camera.output_height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, images[target])) { stop("Unity Vulkan shared image creation failed; the bridge stopped."); return; }
        acquire_image(graphics, slot.vk(0), target, images[target], false);
        copy(slot.vk(0), texture.image, texture.layout, texture.aspect, images[target].image(), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, packet.camera.output_width, packet.camera.output_height);
        release_image(graphics, slot.vk(0), images[target], false);
    } else for (uint32_t i = 0; i < 4; ++i) {
        if (probe && (i == 1 || i == 2)) continue;
        uint32_t width = i == 3 ? packet.camera.output_width : packet.camera.render_width;
        uint32_t height = i == 3 ? packet.camera.output_height : packet.camera.render_height;
        if (!create_image(i, width, height, formats[i], dx_formats[i], images[i])) { stop("Unity Vulkan shared image creation failed; the bridge stopped."); return; }
        acquire_image(graphics, slot.vk(0), i, images[i], false);
        copy(slot.vk(0), textures[i].image, textures[i].layout, textures[i].aspect,
            images[i].image(), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, width, height);
        release_image(graphics, slot.vk(0), images[i], false);
    }
    if (!upload(graphics, slot)) { stop("Unity Vulkan upload submission failed; the bridge stopped."); return; }
    rsf_game_render_pass pass{}; rsf_unity_build_scalar_pass(&packet, &pass);
    if (reconfigure) { pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0; }
    if (present) {
        pass.role = RSF_GAME_RENDER_WINDOW; pass.color_input = images[5].resource(); pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
        host.render_pass(host.user, graphics.queue(), &pass, 1);
    } else if (hudless) {
        pass.role = RSF_GAME_RENDER_FINAL_SCENE; pass.color_input = images[4].resource();
        dx_barrier(slot.commands(), images[4].resource(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
        host.render_pass(host.user, slot.commands(), &pass, 1);
        dx_barrier(slot.commands(), images[4].resource(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COMMON);
    } else {
        void** pointers[] = {&pass.color_input, &pass.depth, &pass.motion, &pass.color_output};
        for (uint32_t i = 0; i < 4; ++i) if (!(probe && (i == 1 || i == 2))) {
            *pointers[i] = images[i].resource();
            dx_barrier(slot.commands(), images[i].resource(), D3D12_RESOURCE_STATE_COMMON,
                i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        }
        host.render_pass(host.user, slot.commands(), &pass, 1);
        for (uint32_t i = 0; i < 4; ++i) if (!(probe && (i == 1 || i == 2)))
            dx_barrier(slot.commands(), images[i].resource(), i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
        if (!probe && !reconfigure) {
            acquire_image(graphics, slot.vk(1), 3, images[3], true);
            copy(slot.vk(1), images[3].image(), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                textures[3].image, textures[3].layout, packet.camera.output_width, packet.camera.output_height);
            release_image(graphics, slot.vk(1), images[3], true);
        }
    }
    if (!submit(graphics, slot, ring)) { stop("Unity Vulkan render submission failed; the bridge stopped."); return; }
    host.render_pass(host.user, slot.commands(), &pass, 0);
    rsf_submission_advance(rsf_unity_vulkan_submission_ring(), ring);
} catch (...) { stop("Unity Vulkan queue execution failed; resources remain owned."); }

// Keep the event admitted through the worker's exception handler as well as its queue calls.
void UNITY_INTERFACE_API execute_queue(int, void* data) noexcept
{
    rsf_unity_vulkan_frame frame{}; uint32_t kind = 0, reconfigure = 0, index = 0;
    if (!rsf_unity_vulkan_start_event(data, &frame, &kind, &reconfigure, &index)) return;
    struct Finish { uint32_t index; ~Finish() { rsf_unity_vulkan_finish_event(index); } } finish{index};
    execute_admitted(frame, kind, reconfigure, index);
}

LRESULT CALLBACK window_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_NCHITTEST) return HTTRANSPARENT;
    if (message == WM_MOUSEACTIVATE) return MA_NOACTIVATE;
    if (message == WM_ERASEBKGND) return 1;
    return DefWindowProcW(hwnd, message, wparam, lparam);
}
BOOL CALLBACK find_window(HWND candidate, LPARAM result)
{
    wchar_t name[64]{}; DWORD process = 0;
    GetWindowThreadProcessId(candidate, &process);
    if (process == GetCurrentProcessId() && GetClassNameW(candidate, name, 64) && std::wcscmp(name, L"UnityWndClass") == 0) {
        *reinterpret_cast<HWND*>(result) = candidate; return FALSE;
    }
    return TRUE;
}
void* create_chain(const rsf_unity_vulkan_device_handles* shared, void* window, uint32_t width, uint32_t height) noexcept
{
    ComPtr<IDXGIFactory2> factory;
    ComPtr<IDXGISwapChain1> created;
    ComPtr<IDXGISwapChain3> chain;
    DXGI_SWAP_CHAIN_DESC1 desc{}; desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->CreateSwapChainForHwnd(DeviceView{*shared}.queue(), static_cast<HWND>(window), &desc, nullptr, nullptr, &created)) ||
        FAILED(created.As(&chain))) return nullptr;
    return chain.Detach();
}
constexpr rsf_unity_vulkan_surface_ops surface_ops{
    create_chain,
    [](void* chain, uint32_t* width, uint32_t* height) noexcept {
        DXGI_SWAP_CHAIN_DESC1 desc{};
        static_cast<IDXGISwapChain3*>(chain)->GetDesc1(&desc);
        *width = desc.Width; *height = desc.Height;
    },
    [](const rsf_unity_vulkan_device_handles* shared) noexcept { return drain(DeviceView{*shared}); },
    [](void* chain, uint32_t width, uint32_t height) noexcept {
        return SUCCEEDED(static_cast<IDXGISwapChain3*>(chain)->ResizeBuffers(3, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0));
    },
    [](void* chain) noexcept { static_cast<IDXGISwapChain3*>(chain)->Release(); },
};
VkResult present_admitted(PFN_vkQueuePresentKHR original_present, VkQueue vk_queue, const VkPresentInfoKHR* info) noexcept try
{
    rsf_unity_vulkan_frame frame{};
    DeviceView graphics;
    const auto unchanged = [&] { return original_present(vk_queue, info); };
    const HWND window = surface_window();
    if (!window || !unity || info->swapchainCount != 1 || !info->pSwapchains || !info->pImageIndices)
        return unchanged();
    const auto instance = unity->Instance();
    if (instance.graphicsQueue != vk_queue) return unchanged();
    // Pair the packet with the native swapchain before creating resources or reporting a source.
    rsf_unity_vulkan_present_source source{};
    const uint32_t target = rsf_unity_vulkan_present_begin(handle_id(instance.instance), instance.device,
        handle_id(vk_queue), handle_id(info->pSwapchains[0]), info->pImageIndices[0], GetParent(window), &source, &frame);
    PresentationLease lease;
    if (!target) return unchanged();
    const auto native = [&] {
        ShowWindowAsync(window, SW_HIDE);
        return unchanged();
    };
    if (target != 1 || !frame.services.render_pass ||
        initialize(graphics, &frame.services) != RSF_UNITY_VULKAN_DEVICE_READY ||
        graphics.vk_device() != instance.device || graphics.vk_queue() != vk_queue) return native();
    auto* chain = static_cast<IDXGISwapChain3*>(rsf_unity_vulkan_configure_surface(source.width, source.height, &surface_ops));
    if (!chain) return native();
    const VkImage presented = from_id<VkImage>(source.image);
    const VkSemaphore present_ready = from_id<VkSemaphore>(source.present_ready);
    ImageView image;
    if (!create_image(5, source.width, source.height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, image)) return native();
    // Preflight before the upload consumes Unity's present waits.
    ComPtr<ID3D12Resource> buffer;
    if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&buffer)))) {
        stop("Unity Vulkan swap chain buffer was unavailable; the bridge stopped.");
        return native();
    }
    const uint32_t ring = rsf_submission_acquire(rsf_unity_vulkan_submission_ring(), nullptr);
    SlotView slot;
    if (!rsf_unity_vulkan_command_slot(ring, &slot.handles) || !begin(graphics, slot, ring)) {
        stop("Unity Vulkan command slot could not be reused; the bridge stopped.");
        return native();
    }
    acquire_image(graphics, slot.vk(0), 5, image, false);
    image_barrier(slot.vk(0), presented, VK_IMAGE_LAYOUT_PRESENT_SRC_KHR, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, 0, VK_ACCESS_TRANSFER_READ_BIT);
    VkImageBlit blit{}; blit.srcSubresource = blit.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    blit.srcOffsets[1] = blit.dstOffsets[1] = {static_cast<int32_t>(source.width), static_cast<int32_t>(source.height), 1};
    vkCmdBlitImage(slot.vk(0), presented, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, image.image(), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &blit, VK_FILTER_NEAREST);
    image_barrier(slot.vk(0), presented, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, VK_ACCESS_TRANSFER_READ_BIT, 0);
    release_image(graphics, slot.vk(0), image, false);
    if (!upload(graphics, slot, info->pWaitSemaphores, info->waitSemaphoreCount)) { stop("Unity Vulkan present upload failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    dx_barrier(slot.commands(), image.resource(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE);
    dx_barrier(slot.commands(), buffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    slot.commands()->CopyResource(buffer.Get(), image.resource());
    dx_barrier(slot.commands(), buffer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    dx_barrier(slot.commands(), image.resource(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
    rsf_game_render_pass pass{}; rsf_unity_build_scalar_pass(&frame.packet, &pass); pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = chain;
    frame.services.render_pass(frame.services.user, graphics.queue(), &pass, 1);
    if (!submit(graphics, slot, ring, present_ready)) { stop("Unity Vulkan present submission failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    if (SUCCEEDED(chain->Present(0, 0))) ShowWindowAsync(window, SW_SHOWNOACTIVATE);
    auto forwarded = *info; forwarded.waitSemaphoreCount = 1; forwarded.pWaitSemaphores = &present_ready;
    rsf_submission_advance(rsf_unity_vulkan_submission_ring(), ring);
    return original_present(vk_queue, &forwarded);
} catch (...) { stop("Unity Vulkan present interception failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
VKAPI_ATTR VkResult VKAPI_CALL intercept_present(VkQueue vk_queue, const VkPresentInfoKHR* info) noexcept try
{
    // Unlike vkGetDeviceProcAddr, the loader export accepts any queue and dispatches to
    // its owning device. A present on a different queue must retain that native path.
    const PFN_vkQueuePresentKHR original_present = vkQueuePresentKHR;
    if (!info) return VK_ERROR_INITIALIZATION_FAILED;
    BridgeEntry entry;
    return entry.admitted ? present_admitted(original_present, vk_queue, info) : original_present(vk_queue, info);
} catch (...) { return VK_ERROR_DEVICE_LOST; }

PFN_vkGetInstanceProcAddr UNITY_INTERFACE_API initialize_intercept(PFN_vkGetInstanceProcAddr original, void*)
{
    const VulkanHooks hooks{intercept_present, vkGetDeviceProcAddr, vkEnumerateDeviceExtensionProperties,
        vkCreateDevice, vkCreateSemaphore, vkDestroySemaphore, vkDeviceWaitIdle, []() noexcept {
            stop("Unity Vulkan could not retire its present semaphores; ownership was retained.");
        }};
    return rsf_unity_vk_install(original, &hooks);
}
}

void rsf_uvk_preload(IUnityInterfaces* interfaces)
{
    unity = interfaces->Get<IUnityGraphicsVulkanV2>();
    if (unity) (void)unity->AddInterceptInitialization(initialize_intercept, nullptr, 0);
}
void rsf_uvk_load(IUnityInterfaces* interfaces, int event_id)
{
    unity = interfaces->Get<IUnityGraphicsVulkanV2>(); base_event = event_id;
    if (!unity) return;
    UnityVulkanPluginEventConfig config{}; config.renderPassPrecondition = kUnityVulkanRenderPass_EnsureOutside;
    config.graphicsQueueAccess = kUnityVulkanGraphicsQueueAccess_DontCare;
    config.flags = kUnityVulkanEventConfigFlag_SyncWorkerThreads;
    for (int offset = 0; offset < 3; ++offset) unity->ConfigureEvent(base_event + offset, &config);
}
void rsf_uvk_unload() noexcept { unity = nullptr; }
bool rsf_uvk_create_window()
{
    HWND window = surface_window();
    if (!window) {
        HWND parent = nullptr; EnumWindows(find_window, reinterpret_cast<LPARAM>(&parent));
        if (!parent || GetWindowThreadProcessId(parent, nullptr) != GetCurrentThreadId()) return false;
        WNDCLASSW type{}; type.lpfnWndProc = window_proc; type.hInstance = GetModuleHandleW(nullptr); type.lpszClassName = L"ReScaleFrame.Unity.Presentation";
        if (!RegisterClassW(&type) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return false;
        RECT client{}; GetClientRect(parent, &client);
        window = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, type.lpszClassName, L"", WS_CHILD | WS_CLIPSIBLINGS,
            0, 0, client.right, client.bottom, parent, nullptr, type.hInstance, nullptr);
        if (!window) return false;
        if (!rsf_unity_vulkan_set_surface_window(window)) { DestroyWindow(window); window = surface_window(); }
    }
    // A release keeps the window but destroys its swap chain, so a later start creates it again
    // instead of reporting the surface ready without one.
    RECT client{};
    return window && GetClientRect(window, &client) &&
        rsf_uvk_update_window(static_cast<uint32_t>(client.right), static_cast<uint32_t>(client.bottom));
}
bool rsf_uvk_owns_window(void* candidate)
{
    const HWND window = surface_window();
    return window && candidate == window;
}
bool rsf_uvk_graphics(ComPtr<ID3D12Device>& device, ComPtr<ID3D12CommandQueue>& queue) noexcept
{
    BridgeEntry entry;
    DeviceView graphics;
    if (!entry.admitted || !rsf_unity_vulkan_device_view(&graphics.handles)) return false;
    device = graphics.device();
    queue = graphics.queue();
    return true;
}
bool rsf_uvk_update_window(uint32_t width, uint32_t height)
{
    const HWND window = surface_window();
    if (!window || !width || !height || GetWindowThreadProcessId(window, nullptr) != GetCurrentThreadId()) return false;
    RECT client{}; if (!GetClientRect(window, &client)) return false;
    if ((client.right != static_cast<LONG>(width) || client.bottom != static_cast<LONG>(height)) &&
        !SetWindowPos(window, nullptr, 0, 0, static_cast<int>(width), static_cast<int>(height), SWP_NOACTIVATE | SWP_NOZORDER)) return false;
    // Establish the shared SDK owner before SR can register a standalone instance. Initial
    // DXGI creation runs on the window thread; subsequent GPU-drained resize stays in Present.
    BridgeEntry entry;
    if (!entry.admitted) return false;
    if (rsf_unity_vulkan_surface_ready()) return true;
    // The swap chain is created under the presentation lease. The window thread does not wait for
    // a present interception holding it; that Present creates the swap chain itself.
    PresentationLease lease{rsf_unity_vulkan_presentation_try_begin() != 0};
    DeviceView graphics;
    return lease.held && (rsf_unity_vulkan_surface_ready() ||
        (initialize(graphics) == RSF_UNITY_VULKAN_DEVICE_READY &&
            rsf_unity_vulkan_configure_surface(width, height, &surface_ops) != nullptr));
}
bool rsf_uvk_render(int event_id, const rsf_unity_packet& packet, const rsf_game_host_services& services, bool reconfigure)
{
    BridgeEntry entry;
    if (!entry.admitted || !unity || !services.render_pass || rsf_unity_vulkan_faulted()) return false;
    const rsf_unity_vulkan_frame frame{packet, services};
    const uint32_t kind = event_id == base_event + 1 ? RSF_UNITY_VULKAN_EVENT_WINDOW :
        event_id == base_event + 2 ? RSF_UNITY_VULKAN_EVENT_HUDLESS : RSF_UNITY_VULKAN_EVENT_SR;
    if (kind == RSF_UNITY_VULKAN_EVENT_WINDOW && !rsf_unity_package_is_editor()) {
        const auto instance = unity->Instance();
        return rsf_unity_vulkan_set_window(&frame, handle_id(instance.instance), instance.device, handle_id(instance.graphicsQueue),
            GetParent(surface_window())) != 0;
    }
    std::array<UnityVulkanImage, 4> textures{};
    void* pointers[] = {packet.color, packet.depth, packet.motion, packet.output};
    for (uint32_t i = 0; i < 4; ++i) {
        if (!pointers[i]) continue;
        const bool output = i == 3;
        if (!unity->AccessTexture(pointers[i], UnityVulkanWholeImage,
            output ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            VK_PIPELINE_STAGE_TRANSFER_BIT, output ? (VK_ACCESS_TRANSFER_WRITE_BIT | VK_ACCESS_TRANSFER_READ_BIT) : VK_ACCESS_TRANSFER_READ_BIT,
            kUnityVulkanResourceAccess_PipelineBarrier, &textures[i])) { log(&services, "Unity Vulkan texture access was refused."); return false; }
    }
    if (!textures[0].image) return false;
    // The event stays outstanding from here until its queue callback finishes.
    uint32_t index = 0;
    void* address = rsf_unity_vulkan_admit_event(&frame, kind, reconfigure ? 1u : 0u, &index);
    if (!address) return false;
    event_images[index] = textures;
    unity->AccessQueue(execute_queue, event_id, address, true);
    return true;
}
