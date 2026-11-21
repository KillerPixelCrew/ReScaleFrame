// SPDX-License-Identifier: MIT
// The Unity package's Vulkan bridge: Unity's Vulkan images are copied into same-adapter D3D12
// resources for the render pass, and Unity's present is replaced by this bridge's DXGI swap chain.
// Shared device readiness, the fault, queued render events and their packets, present and surface
// entries, the window frame identity, whether the DXGI swap chain exists and the checked release
// are Rust (vulkan_state.rs), as are the intercepted swapchains with their images and present-ready
// semaphores, the presentation window's identity and the presentation lease. The command slot
// rotation and shared fence values are the package bridge's submission ring (bridge_interop.rs).
// This file keeps the Unity interfaces, Vulkan, D3D12 and DXGI objects and calls, images, command
// buffers, slot fences, waits, copies, barriers, swapchain and semaphore creation and destruction,
// the window and its swap chain.
#include "vulkan_bridge.h"
#include "vulkan_state.h"
#include "bridge_interop.h"
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
#include <algorithm>
#include <cstring>
#include <vector>

#include "../../../games/unity-mono/src/vulkan_resolver.h"

static_assert(sizeof(rsf_unity_vulkan_present_source) == 24, "vulkan_state.rs PresentSource");
static_assert(sizeof(rsf_unity_vulkan_frame) == sizeof(rsf_unity_packet) + sizeof(rsf_game_host_services));
static_assert(offsetof(rsf_unity_vulkan_frame, services) == sizeof(rsf_unity_packet));

namespace {
using Microsoft::WRL::ComPtr;
IUnityGraphicsVulkanV2* unity = nullptr;
UnityVulkanInstance instance{};
int base_event = 0;
template <class Function> Function saved_proc(uint32_t slot)
{
    return reinterpret_cast<Function>(rsf_unity_vk_resolver_original(slot));
}
ComPtr<ID3D12Device> device;
ComPtr<ID3D12CommandQueue> queue;
ComPtr<ID3D12Fence> shared_fence;
ComPtr<IDXGISwapChain3> chain;
VkSemaphore semaphore = VK_NULL_HANDLE;
VkCommandPool command_pool = VK_NULL_HANDLE;
struct Image {
    ComPtr<ID3D12Resource> resource;
    VkImage image = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
};
std::array<Image, 6> images;
struct Slot {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    std::array<VkCommandBuffer, 2> vk{};
    VkFence complete = VK_NULL_HANDLE;
};
// Indexed by the submission ring's slot (bridge_interop.h).
std::array<Slot, RSF_UNITY_FRAMES_SLOTS> slots;
// Unity's images for each queued render event, indexed by its event slot (vulkan_state.h).
std::array<std::array<UnityVulkanImage, 4>, RSF_UNITY_FRAMES_PACKETS> event_images{};
// Vulkan non-dispatchable handles cross to vulkan_state.rs as 64-bit identity.
template <class Handle> uint64_t handle_id(Handle handle) { return reinterpret_cast<uint64_t>(handle); }
template <class Handle> Handle from_id(uint64_t id) { return reinterpret_cast<Handle>(id); }
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
    explicit BridgeEntry(bool acquired) : admitted(acquired) {}
    BridgeEntry(const BridgeEntry&) = delete;
    BridgeEntry& operator=(const BridgeEntry&) = delete;
    ~BridgeEntry() { if (admitted) rsf_unity_vulkan_leave(); }
};
void dx_barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}
bool drain()
{
    if (!instance.device) return true;
    if (vkQueueWaitIdle(instance.graphicsQueue) != VK_SUCCESS) return false;
    if (shared_fence && !rsf_unity_interop_drained(shared_fence->GetCompletedValue())) {
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
        const bool done = SUCCEEDED(shared_fence->SetEventOnCompletion(rsf_unity_interop_submitted(), event)) && WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
        CloseHandle(event); return done;
    }
    return true;
}
void destroy_image(Image& image)
{
    if (image.image) vkDestroyImage(instance.device, image.image, nullptr);
    if (image.memory) vkFreeMemory(instance.device, image.memory, nullptr);
    image = {};
}
// Destroys the shared device's objects. Only their owner calls it: the device's creator before it
// reports a failure, or release while the bridge is closed.
void destroy_objects()
{
    for (auto& image : images) destroy_image(image);
    for (auto& slot : slots) { if (slot.complete) vkDestroyFence(instance.device, slot.complete, nullptr); slot = {}; }
    if (command_pool) vkDestroyCommandPool(instance.device, command_pool, nullptr);
    if (semaphore) vkDestroySemaphore(instance.device, semaphore, nullptr);
    command_pool = VK_NULL_HANDLE; semaphore = VK_NULL_HANDLE; chain.Reset(); shared_fence.Reset(); queue.Reset(); device.Reset();
    instance = {};
}
bool replace_image(uint32_t index, uint32_t width, uint32_t height,
    uint32_t format_value, uint32_t dx_format_value) noexcept
{
    auto& image = images[index];
    const auto format = static_cast<VkFormat>(format_value);
    const auto dx_format = static_cast<DXGI_FORMAT>(dx_format_value);
    if (!drain()) return false;
    Image next;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC description{}; description.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    description.Width = width; description.Height = height; description.DepthOrArraySize = description.MipLevels = 1;
    description.Format = dx_format; description.SampleDesc.Count = 1;
    description.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &description, D3D12_RESOURCE_STATE_COMMON,
        nullptr, IID_PPV_ARGS(&next.resource)))) return false;
    VkExternalMemoryImageCreateInfo external{VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    external.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkImageCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}; info.pNext = &external;
    info.imageType = VK_IMAGE_TYPE_2D; info.format = format; info.extent = {width, height, 1};
    info.mipLevels = info.arrayLayers = 1; info.samples = VK_SAMPLE_COUNT_1_BIT; info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    if (vkCreateImage(instance.device, &info, nullptr, &next.image) != VK_SUCCESS) return false;
    VkMemoryRequirements requirements{}; vkGetImageMemoryRequirements(instance.device, next.image, &requirements);
    VkPhysicalDeviceMemoryProperties properties{}; vkGetPhysicalDeviceMemoryProperties(instance.physicalDevice, &properties);
    uint32_t memory_type = UINT32_MAX;
    for (uint32_t i = 0; i < properties.memoryTypeCount; ++i)
        if ((requirements.memoryTypeBits & (1u << i)) && (properties.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { memory_type = i; break; }
    HANDLE handle = nullptr;
    if (memory_type == UINT32_MAX || FAILED(device->CreateSharedHandle(next.resource.Get(), nullptr, GENERIC_ALL, nullptr, &handle))) { destroy_image(next); return false; }
    VkMemoryDedicatedAllocateInfo dedicated{VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO}; dedicated.image = next.image;
    VkImportMemoryWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR};
    import.pNext = &dedicated; import.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT; import.handle = handle;
    VkMemoryAllocateInfo allocation{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocation.pNext = &import;
    allocation.allocationSize = requirements.size; allocation.memoryTypeIndex = memory_type;
    VkDeviceMemory allocated_memory = VK_NULL_HANDLE;
    const auto allocated = vkAllocateMemory(instance.device, &allocation, nullptr, &allocated_memory);
    CloseHandle(handle);
    if (allocated != VK_SUCCESS) { destroy_image(next); return false; }
    next.memory = allocated_memory;
    if (vkBindImageMemory(instance.device, next.image, next.memory, 0) != VK_SUCCESS) {
        destroy_image(next); return false;
    }
    destroy_image(image); image = std::move(next); return true;
}

bool create_image(uint32_t index, uint32_t width, uint32_t height, VkFormat format, DXGI_FORMAT dx_format)
{
    return rsf_unity_vulkan_prepare_image(index, width, height,
        static_cast<uint32_t>(format), static_cast<uint32_t>(dx_format), replace_image);
}
// Creates the shared D3D12 device, queue, fence, imported semaphore and command slots. A failure
// may leave some of them; the caller destroys them.
bool create_shared(const rsf_game_host_services* services)
{
    instance = unity->Instance(); if (!instance.device) return false;
    const bool external_extensions = rsf_unity_vk_interop_enabled(instance.device) ||
        (vkGetDeviceProcAddr(instance.device, "vkImportSemaphoreWin32HandleKHR") &&
            vkGetDeviceProcAddr(instance.device, "vkGetMemoryWin32HandlePropertiesKHR"));
    if (!external_extensions) {
        log(services, "Unity Vulkan device lacks enabled Win32 external-memory/semaphore extensions.");
        return false;
    }
    VkPhysicalDeviceIDProperties id{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; properties.pNext = &id;
    vkGetPhysicalDeviceProperties2(instance.physicalDevice, &properties);
    if (!id.deviceLUIDValid) { log(services, "Unity Vulkan adapter supplied no Windows LUID."); return false; }
    LUID luid{}; std::memcpy(&luid, id.deviceLUID, sizeof(luid));
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&adapter))) ||
        FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) return false;
    D3D12_COMMAND_QUEUE_DESC q{};
    if (FAILED(device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue))) ||
        FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_SHARED, IID_PPV_ARGS(&shared_fence)))) return false;
    HANDLE handle = nullptr;
    if (FAILED(device->CreateSharedHandle(shared_fence.Get(), nullptr, GENERIC_ALL, nullptr, &handle))) return false;
    VkSemaphoreCreateInfo semaphore_info{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    auto result = vkCreateSemaphore(instance.device, &semaphore_info, nullptr, &semaphore);
    const auto import_semaphore = reinterpret_cast<PFN_vkImportSemaphoreWin32HandleKHR>(vkGetDeviceProcAddr(instance.device, "vkImportSemaphoreWin32HandleKHR"));
    VkImportSemaphoreWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_SEMAPHORE_WIN32_HANDLE_INFO_KHR};
    import.semaphore = semaphore; import.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT; import.handle = handle;
    if (result == VK_SUCCESS && import_semaphore) result = import_semaphore(instance.device, &import); else result = VK_ERROR_EXTENSION_NOT_PRESENT;
    CloseHandle(handle); if (result != VK_SUCCESS) return false;
    VkCommandPoolCreateInfo pool{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO}; pool.queueFamilyIndex = instance.queueFamilyIndex;
    pool.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    if (vkCreateCommandPool(instance.device, &pool, nullptr, &command_pool) != VK_SUCCESS) return false;
    for (auto& slot : slots) {
        VkCommandBufferAllocateInfo allocate{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO}; allocate.commandPool = command_pool;
        allocate.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocate.commandBufferCount = 2;
        VkFenceCreateInfo fence{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO}; fence.flags = VK_FENCE_CREATE_SIGNALED_BIT;
        if (vkAllocateCommandBuffers(instance.device, &allocate, slot.vk.data()) != VK_SUCCESS ||
            vkCreateFence(instance.device, &fence, nullptr, &slot.complete) != VK_SUCCESS ||
            FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&slot.allocator))) ||
            FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, slot.allocator.Get(), nullptr, IID_PPV_ARGS(&slot.commands))) ||
            FAILED(slot.commands->Close())) return false;
    }
    log(services, "Unity Vulkan textures share same-adapter D3D12 resources and GPU fences."); return true;
}
enum class SharedDevice { ready, busy, failed };
// The shared device, created by one thread at a time. busy while another thread creates it or
// release closes the bridge; a failed creation leaves nothing behind and is retried by a later call.
SharedDevice initialize(const rsf_game_host_services* services = nullptr)
{
    if (!unity) return SharedDevice::failed;
    const uint32_t claim = rsf_unity_vulkan_claim_device();
    if (claim == RSF_UNITY_VULKAN_DEVICE_READY) return SharedDevice::ready;
    if (claim != RSF_UNITY_VULKAN_DEVICE_CLAIMED) return SharedDevice::busy;
    // Reports on every exit, so the claim never stays open and blocks retirement.
    struct Claim {
        bool created = false;
        ~Claim() { if (!created) destroy_objects(); rsf_unity_vulkan_device_created(created ? 1u : 0u); }
    } claimed;
    claimed.created = create_shared(services);
    return claimed.created ? SharedDevice::ready : SharedDevice::failed;
}
bool begin(Slot& slot)
{
    if (vkWaitForFences(instance.device, 1, &slot.complete, VK_TRUE, 10000000000ull) != VK_SUCCESS) return false;
    for (auto command : slot.vk) {
        if (vkResetCommandBuffer(command, 0) != VK_SUCCESS) return false;
        VkCommandBufferBeginInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        if (vkBeginCommandBuffer(command, &info) != VK_SUCCESS) return false;
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.commands->Reset(slot.allocator.Get(), nullptr)))
        return false;
    rsf_unity_vulkan_begin_recording();
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
void acquire_image(VkCommandBuffer commands, uint32_t index, bool output)
{
    const auto old_layout = rsf_unity_vulkan_record_image(index) ?
        VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED;
    image_barrier(commands, images[index].image, old_layout,
        output ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL : VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_QUEUE_FAMILY_EXTERNAL, instance.queueFamilyIndex, 0, output ? VK_ACCESS_TRANSFER_READ_BIT : VK_ACCESS_TRANSFER_WRITE_BIT);
}
void release_image(VkCommandBuffer commands, Image& image, bool output)
{
    image_barrier(commands, image.image, output ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL : VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_GENERAL, instance.queueFamilyIndex, VK_QUEUE_FAMILY_EXTERNAL,
        output ? VK_ACCESS_TRANSFER_READ_BIT : VK_ACCESS_TRANSFER_WRITE_BIT, 0);
}
// Vulkan signals the next shared fence value after the copies; the D3D12 queue waits for it.
bool upload(Slot& slot, const VkSemaphore* waits = nullptr, uint32_t wait_count = 0)
{
    if (vkEndCommandBuffer(slot.vk[0]) != VK_SUCCESS) return false;
    std::vector<VkPipelineStageFlags> stages(wait_count, VK_PIPELINE_STAGE_TRANSFER_BIT);
    std::vector<uint64_t> values(wait_count, 0);
    const uint64_t value = rsf_unity_interop_next_completion();
    if (!value) return false;
    VkD3D12FenceSubmitInfoKHR fence{VK_STRUCTURE_TYPE_D3D12_FENCE_SUBMIT_INFO_KHR};
    fence.waitSemaphoreValuesCount = wait_count; fence.pWaitSemaphoreValues = values.data();
    fence.signalSemaphoreValuesCount = 1; fence.pSignalSemaphoreValues = &value;
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.pNext = &fence;
    submit.waitSemaphoreCount = wait_count; submit.pWaitSemaphores = waits; submit.pWaitDstStageMask = stages.data();
    submit.commandBufferCount = 1; submit.pCommandBuffers = &slot.vk[0]; submit.signalSemaphoreCount = 1; submit.pSignalSemaphores = &semaphore;
    if (vkQueueSubmit(instance.graphicsQueue, 1, &submit, VK_NULL_HANDLE) != VK_SUCCESS) return false;
    rsf_unity_vulkan_upload_submitted();
    if (rsf_unity_interop_completion() != value) return false;
    return SUCCEEDED(queue->Wait(shared_fence.Get(), value));
}
// The D3D12 queue signals the ring slot's shared fence value; Vulkan waits for it before the
// hand-back copies and signals the slot's fence.
bool submit(Slot& slot, uint32_t ring, VkSemaphore ready = VK_NULL_HANDLE)
{
    if (FAILED(slot.commands->Close()) || vkEndCommandBuffer(slot.vk[1]) != VK_SUCCESS) return false;
    ID3D12CommandList* lists[] = {slot.commands.Get()}; queue->ExecuteCommandLists(1, lists);
    const uint64_t value = rsf_unity_interop_submit(ring);
    if (!value || FAILED(queue->Signal(shared_fence.Get(), value))) return false;
    VkD3D12FenceSubmitInfoKHR fence{VK_STRUCTURE_TYPE_D3D12_FENCE_SUBMIT_INFO_KHR};
    fence.waitSemaphoreValuesCount = 1; fence.pWaitSemaphoreValues = &value;
    uint64_t zero = 0; fence.signalSemaphoreValuesCount = ready ? 1u : 0u; fence.pSignalSemaphoreValues = &zero;
    VkPipelineStageFlags stage = VK_PIPELINE_STAGE_TRANSFER_BIT;
    VkSubmitInfo submit_info{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit_info.pNext = &fence;
    submit_info.waitSemaphoreCount = 1; submit_info.pWaitSemaphores = &semaphore; submit_info.pWaitDstStageMask = &stage;
    submit_info.commandBufferCount = 1; submit_info.pCommandBuffers = &slot.vk[1];
    submit_info.signalSemaphoreCount = ready ? 1u : 0u; submit_info.pSignalSemaphores = &ready;
    if (vkResetFences(instance.device, 1, &slot.complete) != VK_SUCCESS) return false;
    if (vkQueueSubmit(instance.graphicsQueue, 1, &submit_info, slot.complete) != VK_SUCCESS) return false;
    rsf_unity_vulkan_handback_fenced();
    return true;
}
void copy(VkCommandBuffer commands, VkImage source, VkImageLayout source_layout, VkImageAspectFlags aspect,
    VkImage target, VkImageLayout target_layout, uint32_t width, uint32_t height)
{
    VkImageCopy region{}; region.srcSubresource = {aspect, 0, 0, 1}; region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {width, height, 1}; vkCmdCopyImage(commands, source, source_layout, target, target_layout, 1, &region);
}
void make_pass(const rsf_unity_packet& packet, rsf_game_render_pass& pass)
{
    pass = {}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = packet.session_id; pass.family_key = pass.view_key = pass.history_key = packet.view_key;
    pass.native_frame = pass.source_frame_id = packet.frame_id; pass.resource_generation = packet.generation;
    pass.screen = RSF_SCREEN_FLIGHT; pass.flags = packet.flags & RSF_UNITY_PACKET_RESET;
    pass.camera = packet.camera; pass.camera_valid = (packet.flags & RSF_UNITY_PACKET_PROBE) ? 0u : 1u;
    pass.render_rect[2] = static_cast<int32_t>(packet.camera.render_width); pass.render_rect[3] = static_cast<int32_t>(packet.camera.render_height);
    pass.output_rect[2] = static_cast<int32_t>(packet.camera.output_width); pass.output_rect[3] = static_cast<int32_t>(packet.camera.output_height);
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = 1; pass.motion_camera_included = 1;
    std::memcpy(pass.previous_clip_to_clip, packet.previous_clip_to_clip, sizeof(pass.previous_clip_to_clip));
}
// Unity's queue access for one admitted render event; data is the event's Rust address.
void UNITY_INTERFACE_API execute_queue(int, void* data) noexcept try
{
    rsf_unity_vulkan_frame frame{}; uint32_t kind = 0, reconfigure = 0, index = 0;
    // A faulted bridge discards the event here.
    if (!rsf_unity_vulkan_start_event(data, &frame, &kind, &reconfigure, &index)) return;
    struct Finish { uint32_t index; ~Finish() { rsf_unity_vulkan_finish_event(index); } } finish{index};
    PresentationLease lease{rsf_unity_vulkan_presentation_try_begin() != 0};
    if (!lease.held) return;
    if (index >= event_images.size() || !frame.services.render_pass) return;
    const auto& host = frame.services;
    const auto& packet = frame.packet;
    const auto& textures = event_images[index];
    const bool present = kind == RSF_UNITY_VULKAN_EVENT_WINDOW, hudless = kind == RSF_UNITY_VULKAN_EVENT_HUDLESS;
    const SharedDevice ready = initialize(&host);
    if (ready == SharedDevice::busy) return;
    if (ready != SharedDevice::ready) {
        fault(RSF_BACKEND_ERROR_INIT_FAILED, "Unity Vulkan shared device/fence initialization failed; native output remains in place.");
        return;
    }
    const uint32_t ring = rsf_unity_interop_slot(nullptr);
    if (ring >= slots.size()) return;
    auto& slot = slots[ring]; if (!begin(slot)) { stop("Unity Vulkan command slot could not be reused; the bridge stopped."); return; }
    if (reconfigure && !drain()) { stop("Unity Vulkan GPU drain before reconfiguration failed; the bridge stopped."); return; }
    constexpr VkFormat formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
    constexpr DXGI_FORMAT dx_formats[] = {DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT};
    const bool probe = (packet.flags & RSF_UNITY_PACKET_PROBE) != 0;
    if (hudless || present) {
        const auto& texture = textures[0];
        const uint32_t target = present ? 5u : 4u;
        if (!create_image(target, packet.camera.output_width, packet.camera.output_height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM)) { stop("Unity Vulkan shared image creation failed; the bridge stopped."); return; }
        acquire_image(slot.vk[0], target, false);
        copy(slot.vk[0], texture.image, texture.layout, texture.aspect, images[target].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, packet.camera.output_width, packet.camera.output_height);
        release_image(slot.vk[0], images[target], false);
    } else for (uint32_t i = 0; i < 4; ++i) {
        if (probe && (i == 1 || i == 2)) continue;
        uint32_t width = i == 3 ? packet.camera.output_width : packet.camera.render_width;
        uint32_t height = i == 3 ? packet.camera.output_height : packet.camera.render_height;
        if (!create_image(i, width, height, formats[i], dx_formats[i])) { stop("Unity Vulkan shared image creation failed; the bridge stopped."); return; }
        acquire_image(slot.vk[0], i, false);
        copy(slot.vk[0], textures[i].image, textures[i].layout, textures[i].aspect,
            images[i].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, width, height);
        release_image(slot.vk[0], images[i], false);
    }
    if (!upload(slot)) { stop("Unity Vulkan upload submission failed; the bridge stopped."); return; }
    rsf_game_render_pass pass{}; make_pass(packet, pass);
    if (reconfigure) { pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0; }
    if (present) {
        pass.role = RSF_GAME_RENDER_WINDOW; pass.color_input = images[5].resource.Get(); pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
        host.render_pass(host.user, queue.Get(), &pass, 1);
    } else if (hudless) {
        pass.role = RSF_GAME_RENDER_FINAL_SCENE; pass.color_input = images[4].resource.Get();
        dx_barrier(slot.commands.Get(), images[4].resource.Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
        host.render_pass(host.user, slot.commands.Get(), &pass, 1);
        dx_barrier(slot.commands.Get(), images[4].resource.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COMMON);
    } else {
        void** pointers[] = {&pass.color_input, &pass.depth, &pass.motion, &pass.color_output};
        for (uint32_t i = 0; i < 4; ++i) if (!(probe && (i == 1 || i == 2))) {
            *pointers[i] = images[i].resource.Get();
            dx_barrier(slot.commands.Get(), images[i].resource.Get(), D3D12_RESOURCE_STATE_COMMON,
                i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        }
        host.render_pass(host.user, slot.commands.Get(), &pass, 1);
        for (uint32_t i = 0; i < 4; ++i) if (!(probe && (i == 1 || i == 2)))
            dx_barrier(slot.commands.Get(), images[i].resource.Get(), i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
        if (!probe && !reconfigure) {
            acquire_image(slot.vk[1], 3, true);
            copy(slot.vk[1], images[3].image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                textures[3].image, textures[3].layout, packet.camera.output_width, packet.camera.output_height);
            release_image(slot.vk[1], images[3], true);
        }
    }
    if (!submit(slot, ring)) { stop("Unity Vulkan render submission failed; the bridge stopped."); return; }
    host.render_pass(host.user, slot.commands.Get(), &pass, 0);
    rsf_unity_interop_advance(ring);
} catch (...) { stop("Unity Vulkan queue execution failed; resources remain owned."); }

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
// Creates or resizes the bridge's swap chain, with the presentation lease and an entry held.
bool configure_chain(HWND window, uint32_t width, uint32_t height)
{
    if (!window) return false;
    if (!chain) {
        ComPtr<IDXGIFactory2> factory; ComPtr<IDXGISwapChain1> created;
        DXGI_SWAP_CHAIN_DESC1 desc{}; desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->CreateSwapChainForHwnd(queue.Get(), window, &desc, nullptr, nullptr, &created)) ||
            FAILED(created.As(&chain))) return false;
        rsf_unity_vulkan_surface_created();
    } else {
        DXGI_SWAP_CHAIN_DESC1 desc{}; chain->GetDesc1(&desc);
        if ((desc.Width != width || desc.Height != height) &&
            (!drain() || FAILED(chain->ResizeBuffers(3, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0)))) return false;
    }
    return true;
}
VKAPI_ATTR VkResult VKAPI_CALL intercept_present(VkQueue vk_queue, const VkPresentInfoKHR* info) noexcept try
{
    const auto original_present = saved_proc<PFN_vkQueuePresentKHR>(RSF_UNITY_VK_PRESENT);
    if (!original_present || !info) return VK_ERROR_INITIALIZATION_FAILED;
    BridgeEntry entry; rsf_unity_vulkan_frame frame{};
    const auto unchanged = [&] { return original_present(vk_queue, info); };
    const HWND window = surface_window();
    if (!window || !entry.admitted || !rsf_unity_vulkan_window(&frame) || !frame.services.render_pass || info->swapchainCount != 1 ||
        initialize(&frame.services) != SharedDevice::ready) return unchanged();
    // The lease is held from the lookup through the forwarded Present, also when the swapchain is
    // unknown or the index out of range and the Present is forwarded unchanged.
    rsf_unity_vulkan_present_source source{};
    const bool found = rsf_unity_vulkan_present_begin(handle_id(info->pSwapchains[0]), info->pImageIndices[0], &source) != 0;
    PresentationLease lease;
    const VkImage presented = from_id<VkImage>(source.image);
    const VkSemaphore present_ready = from_id<VkSemaphore>(source.present_ready);
    if (!found || !configure_chain(window, source.width, source.height) ||
        !create_image(5, source.width, source.height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM)) return unchanged();
    const uint32_t ring = rsf_unity_interop_slot(nullptr);
    if (ring >= slots.size()) return unchanged();
    auto& slot = slots[ring]; if (!begin(slot)) return unchanged();
    acquire_image(slot.vk[0], 5, false);
    image_barrier(slot.vk[0], presented, VK_IMAGE_LAYOUT_PRESENT_SRC_KHR, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, 0, VK_ACCESS_TRANSFER_READ_BIT);
    VkImageBlit blit{}; blit.srcSubresource = blit.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    blit.srcOffsets[1] = blit.dstOffsets[1] = {static_cast<int32_t>(source.width), static_cast<int32_t>(source.height), 1};
    vkCmdBlitImage(slot.vk[0], presented, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, images[5].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &blit, VK_FILTER_NEAREST);
    image_barrier(slot.vk[0], presented, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, VK_ACCESS_TRANSFER_READ_BIT, 0);
    release_image(slot.vk[0], images[5], false);
    if (!upload(slot, info->pWaitSemaphores, info->waitSemaphoreCount)) { stop("Unity Vulkan present upload failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    ComPtr<ID3D12Resource> buffer;
    if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&buffer)))) { stop("Unity Vulkan swap chain buffer was unavailable; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    dx_barrier(slot.commands.Get(), images[5].resource.Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE);
    dx_barrier(slot.commands.Get(), buffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    slot.commands->CopyResource(buffer.Get(), images[5].resource.Get());
    dx_barrier(slot.commands.Get(), buffer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    dx_barrier(slot.commands.Get(), images[5].resource.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
    rsf_game_render_pass pass{}; make_pass(frame.packet, pass); pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = chain.Get();
    frame.services.render_pass(frame.services.user, queue.Get(), &pass, 1);
    if (!submit(slot, ring, present_ready)) { stop("Unity Vulkan present submission failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    if (SUCCEEDED(chain->Present(0, 0))) ShowWindowAsync(window, SW_SHOWNOACTIVATE);
    auto forwarded = *info; forwarded.waitSemaphoreCount = 1; forwarded.pWaitSemaphores = &present_ready;
    rsf_unity_interop_advance(ring);
    return original_present(vk_queue, &forwarded);
} catch (...) { stop("Unity Vulkan present interception failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
struct DeviceRequest {
    VkPhysicalDevice physical;
    const VkDeviceCreateInfo* info;
    const VkAllocationCallbacks* allocator;
    VkDevice* output;
};

bool device_extensions(void* context, const char* const* names, uint32_t name_count) noexcept try
{
    const auto& request = *static_cast<const DeviceRequest*>(context);
    uint32_t count = 0;
    if (vkEnumerateDeviceExtensionProperties(request.physical, nullptr, &count, nullptr) != VK_SUCCESS)
        return false;
    std::vector<VkExtensionProperties> available(count);
    if (vkEnumerateDeviceExtensionProperties(request.physical, nullptr, &count, available.data()) != VK_SUCCESS ||
        count > available.size()) return false;
    available.resize(count);
    for (uint32_t i = 0; i < name_count; ++i) {
        if (std::none_of(available.begin(), available.end(), [&](const auto& item) {
                return std::strcmp(item.extensionName, names[i]) == 0;
            })) return false;
    }
    return true;
} catch (...) { return false; }

int32_t create_device(void* context, PFN_vkVoidFunction original, const char* const* names,
    uint32_t count, void** created) noexcept
{
    const auto& request = *static_cast<const DeviceRequest*>(context);
    const VkDeviceCreateInfo* info = request.info;
    VkDeviceCreateInfo configured{};
    if (names) {
        configured = *info;
        configured.enabledExtensionCount = count;
        configured.ppEnabledExtensionNames = names;
        info = &configured;
    }
    const auto create = reinterpret_cast<PFN_vkCreateDevice>(original);
    const auto result = create(request.physical, info, request.allocator, request.output);
    if (result == VK_SUCCESS) *created = *request.output;
    return result;
}

static_assert(VK_SUCCESS == 0 && VK_ERROR_OUT_OF_HOST_MEMORY == -1 && VK_ERROR_INITIALIZATION_FAILED == -3);

VKAPI_ATTR VkResult VKAPI_CALL intercept_create_device(VkPhysicalDevice physical, const VkDeviceCreateInfo* info,
    const VkAllocationCallbacks* allocator, VkDevice* output) noexcept
{
    if (!info || !output) return VK_ERROR_INITIALIZATION_FAILED;
    DeviceRequest request{physical, info, allocator, output};
    return static_cast<VkResult>(rsf_unity_vk_resolver_create_device(info->ppEnabledExtensionNames,
        info->enabledExtensionCount, &request, device_extensions, create_device));
}
VKAPI_ATTR VkResult VKAPI_CALL intercept_create_swapchain(VkDevice vk_device, const VkSwapchainCreateInfoKHR* info, const VkAllocationCallbacks* allocator, VkSwapchainKHR* output) noexcept try
{
    const auto original_create_swapchain = saved_proc<PFN_vkCreateSwapchainKHR>(RSF_UNITY_VK_CREATE_SWAPCHAIN);
    if (!original_create_swapchain || !info || !output) return VK_ERROR_INITIALIZATION_FAILED;
    auto destroy = saved_proc<PFN_vkDestroySwapchainKHR>(RSF_UNITY_VK_DESTROY_SWAPCHAIN);
    const auto gdpa = saved_proc<PFN_vkGetDeviceProcAddr>(RSF_UNITY_VK_DEVICE_PROC);
    if (!destroy && gdpa)
        destroy = reinterpret_cast<PFN_vkDestroySwapchainKHR>(gdpa(vk_device, "vkDestroySwapchainKHR"));
    if (!destroy) return original_create_swapchain(vk_device, info, allocator, output);

    auto request = *info; request.imageUsage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    std::vector<VkImage> images;
    std::vector<VkSemaphore> present_ready;
    struct PendingSwapchain {
        VkDevice device;
        const VkAllocationCallbacks* allocator;
        PFN_vkDestroySwapchainKHR destroy;
        const std::vector<VkSemaphore>& semaphores;
        VkSwapchainKHR chain = VK_NULL_HANDLE;
        ~PendingSwapchain()
        {
            if (!chain) return;
            for (auto semaphore : semaphores)
                if (semaphore) vkDestroySemaphore(device, semaphore, nullptr);
            destroy(device, chain, allocator);
        }
    } pending{vk_device, allocator, destroy, present_ready};

    *output = VK_NULL_HANDLE;
    VkSwapchainKHR created = VK_NULL_HANDLE;
    const auto result = original_create_swapchain(vk_device, &request, allocator, &created);
    if (result != VK_SUCCESS) return result;
    pending.chain = created;
    uint32_t count = 0;
    if (vkGetSwapchainImagesKHR(vk_device, pending.chain, &count, nullptr) != VK_SUCCESS || !count)
        return VK_ERROR_INITIALIZATION_FAILED;
    images.resize(count);
    if (vkGetSwapchainImagesKHR(vk_device, pending.chain, &count, images.data()) != VK_SUCCESS || !count)
        return VK_ERROR_INITIALIZATION_FAILED;
    images.resize(count);
    present_ready.resize(count);
    VkSemaphoreCreateInfo semaphore_info{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    for (auto& ready : present_ready) {
        VkSemaphore semaphore = VK_NULL_HANDLE;
        if (vkCreateSemaphore(vk_device, &semaphore_info, nullptr, &semaphore) != VK_SUCCESS)
            return VK_ERROR_OUT_OF_DEVICE_MEMORY;
        ready = semaphore;
    }
    std::vector<uint64_t> image_ids(count), ready_ids(count);
    for (uint32_t i = 0; i < count; ++i) { image_ids[i] = handle_id(images[i]); ready_ids[i] = handle_id(present_ready[i]); }
    // A refused publication (a handle already published) rolls back like any other step.
    if (!rsf_unity_vulkan_swapchain_register(handle_id(pending.chain), request.imageExtent.width, request.imageExtent.height,
        image_ids.data(), ready_ids.data(), count)) return VK_ERROR_INITIALIZATION_FAILED;
    *output = pending.chain;
    pending.chain = VK_NULL_HANDLE;
    return result;
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }
// Destroys a retired swapchain's present-ready semaphores. Runs with the presentation lease held, so
// no present interception submits to or presents on the device's queues while vkDeviceWaitIdle
// waits for them.
void retire_present_ready(void* context, const uint64_t* present_ready, uint32_t count) noexcept
{
    const auto vk_device = static_cast<VkDevice>(context);
    vkDeviceWaitIdle(vk_device);
    for (uint32_t i = 0; i < count; ++i) vkDestroySemaphore(vk_device, from_id<VkSemaphore>(present_ready[i]), nullptr);
}
VKAPI_ATTR void VKAPI_CALL intercept_destroy_swapchain(VkDevice vk_device, VkSwapchainKHR swapchain, const VkAllocationCallbacks* allocator)
{
    (void)rsf_unity_vulkan_swapchain_retire(handle_id(swapchain), retire_present_ready, vk_device);
    const auto destroy = saved_proc<PFN_vkDestroySwapchainKHR>(RSF_UNITY_VK_DESTROY_SWAPCHAIN);
    if (destroy) destroy(vk_device, swapchain, allocator);
}

VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL get_device_proc(VkDevice vk_device, const char* name);

PFN_vkVoidFunction intercept(const char* name, PFN_vkVoidFunction original, bool from_instance)
{
    switch (rsf_unity_vk_resolver_route(name, original, from_instance)) {
    case RSF_UNITY_VK_DEVICE_PROC: return reinterpret_cast<PFN_vkVoidFunction>(get_device_proc);
    case RSF_UNITY_VK_CREATE_DEVICE: return reinterpret_cast<PFN_vkVoidFunction>(intercept_create_device);
    case RSF_UNITY_VK_CREATE_SWAPCHAIN: return reinterpret_cast<PFN_vkVoidFunction>(intercept_create_swapchain);
    case RSF_UNITY_VK_DESTROY_SWAPCHAIN: return reinterpret_cast<PFN_vkVoidFunction>(intercept_destroy_swapchain);
    case RSF_UNITY_VK_PRESENT: return reinterpret_cast<PFN_vkVoidFunction>(intercept_present);
    default: return original;
    }
}

VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL get_device_proc(VkDevice vk_device, const char* name)
{
    const auto get = saved_proc<PFN_vkGetDeviceProcAddr>(RSF_UNITY_VK_DEVICE_PROC);
    return get && name ? intercept(name, get(vk_device, name), false) : nullptr;
}

VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL get_instance_proc(VkInstance vk_instance, const char* name)
{
    const auto get = saved_proc<PFN_vkGetInstanceProcAddr>(RSF_UNITY_VK_INSTANCE_PROC);
    return get && name ? intercept(name, get(vk_instance, name), true) : nullptr;
}

PFN_vkGetInstanceProcAddr UNITY_INTERFACE_API initialize_intercept(PFN_vkGetInstanceProcAddr original, void*)
{
    return rsf_unity_vk_bind_resolver(reinterpret_cast<PFN_vkVoidFunction>(original)) ? get_instance_proc : original;
}
// Every slot fence signalled. Slot fences exist only while the shared device is ready, and only
// release replaces them.
bool bridge_fences_retired()
{
    for (const auto& slot : slots)
        if (slot.complete && vkGetFenceStatus(instance.device, slot.complete) != VK_SUCCESS) return false;
    return rsf_unity_interop_drained(shared_fence ? shared_fence->GetCompletedValue() : 0) != 0;
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
bool rsf_uvk_graphics(void** owning_device, void** owning_queue)
{
    BridgeEntry entry;
    if (!entry.admitted || !owning_device || !owning_queue || !rsf_unity_vulkan_ready()) return false;
    *owning_device = device.Get(); *owning_queue = queue.Get(); return true;
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
    return lease.held && (chain || (initialize() == SharedDevice::ready && configure_chain(window, width, height)));
}
bool rsf_uvk_render(int event_id, const rsf_unity_packet& packet, const rsf_game_host_services& services, bool reconfigure)
{
    BridgeEntry entry;
    if (!entry.admitted || !unity || !services.render_pass || rsf_unity_vulkan_faulted()) return false;
    const rsf_unity_vulkan_frame frame{packet, services};
    const uint32_t kind = event_id == base_event + 1 ? RSF_UNITY_VULKAN_EVENT_WINDOW :
        event_id == base_event + 2 ? RSF_UNITY_VULKAN_EVENT_HUDLESS : RSF_UNITY_VULKAN_EVENT_SR;
    if (kind == RSF_UNITY_VULKAN_EVENT_WINDOW && !rsf_unity_package_is_editor()) return rsf_unity_vulkan_set_window(&frame) != 0;
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
bool rsf_uvk_drained()
{
    uint32_t ready = 0;
    BridgeEntry entry{rsf_unity_vulkan_probe_idle(&ready) != 0};
    if (!entry.admitted) return false;
    if (!ready) return true;
    PresentationLease lease{rsf_unity_vulkan_presentation_try_begin() != 0};
    return lease.held && bridge_fences_retired();
}
void rsf_uvk_release()
{
    // Closing refuses new events, present interceptions, surface updates and device creation until
    // the objects are gone or the GPU work is found outstanding.
    if (!rsf_unity_vulkan_close()) return;
    PresentationLease lease{rsf_unity_vulkan_presentation_try_begin() != 0};
    // Lifecycle release never calls Vulkan queue operations. Rust refused close while
    // an upload was missing its hand-back fence, so both bridge fence sets can prove retirement.
    if (!lease.held || !bridge_fences_retired()) { rsf_unity_vulkan_reopen(); return; }
    destroy_objects();
    // The slot ring and shared fence values go with the objects that used them.
    rsf_unity_interop_release();
    rsf_unity_vulkan_release();
}
