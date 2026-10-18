// SPDX-License-Identifier: GPL-3.0-only
#include "vulkan_bridge.h"
#include "package.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsVulkan.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <array>
#include <atomic>
#include <algorithm>
#include <cstring>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
IUnityGraphicsVulkanV2* unity = nullptr;
UnityVulkanInstance instance{};
rsf_game_host_services host{};
int base_event = 0;
std::atomic<uint32_t> callbacks{0};
PFN_vkGetInstanceProcAddr original_gipa = nullptr;
PFN_vkGetDeviceProcAddr original_gdpa = nullptr;
PFN_vkCreateDevice original_create_device = nullptr;
PFN_vkCreateSwapchainKHR original_create_swapchain = nullptr;
PFN_vkDestroySwapchainKHR original_destroy_swapchain = nullptr;
PFN_vkQueuePresentKHR original_present = nullptr;
PFN_vkImportSemaphoreWin32HandleKHR import_semaphore = nullptr;
bool external_extensions = false, faulted = false, initialized = false;
HWND window = nullptr;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12CommandQueue> queue;
ComPtr<ID3D12Fence> shared_fence;
ComPtr<IDXGISwapChain3> chain;
VkSemaphore semaphore = VK_NULL_HANDLE;
VkCommandPool command_pool = VK_NULL_HANDLE;
uint64_t fence_value = 0;
uint32_t next_slot = 0;
rsf_unity_packet last_window{};
struct Image {
    ComPtr<ID3D12Resource> resource;
    VkImage image = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    uint32_t width = 0, height = 0;
    VkFormat format = VK_FORMAT_UNDEFINED;
    bool initialized = false;
};
std::array<Image, 6> images;
struct Slot {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    std::array<VkCommandBuffer, 2> vk{};
    VkFence complete = VK_NULL_HANDLE;
};
std::array<Slot, 3> slots;
struct Swapchain {
    VkDevice device = VK_NULL_HANDLE;
    VkExtent2D extent{};
    VkFormat format = VK_FORMAT_UNDEFINED;
    std::vector<VkImage> images;
    std::vector<VkSemaphore> present_ready;
};
std::mutex swapchain_guard;
std::unordered_map<VkSwapchainKHR, Swapchain> swapchains;
void log(const char* text) { if (host.log) host.log(host.user, text); else { OutputDebugStringA(text); OutputDebugStringA("\n"); } }
void dx_barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}
bool drain()
{
    if (!instance.device) return true;
    if (vkQueueWaitIdle(instance.graphicsQueue) != VK_SUCCESS) return false;
    if (shared_fence && shared_fence->GetCompletedValue() < fence_value) {
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
        const bool done = SUCCEEDED(shared_fence->SetEventOnCompletion(fence_value, event)) && WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
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
bool create_image(uint32_t index, uint32_t width, uint32_t height, VkFormat format, DXGI_FORMAT dx_format)
{
    auto& image = images[index];
    if (image.image && image.width == width && image.height == height && image.format == format) return true;
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
    const auto allocated = vkAllocateMemory(instance.device, &allocation, nullptr, &next.memory); CloseHandle(handle);
    if (allocated != VK_SUCCESS || vkBindImageMemory(instance.device, next.image, next.memory, 0) != VK_SUCCESS) { destroy_image(next); return false; }
    next.width = width; next.height = height; next.format = format;
    destroy_image(image); image = std::move(next); return true;
}
bool initialize()
{
    if (initialized) return true;
    if (!unity) return false;
    instance = unity->Instance(); if (!instance.device) return false;
    if (!external_extensions) {
        external_extensions = vkGetDeviceProcAddr(instance.device, "vkImportSemaphoreWin32HandleKHR") &&
            vkGetDeviceProcAddr(instance.device, "vkGetMemoryWin32HandlePropertiesKHR");
    }
    if (!external_extensions) { log("Unity Vulkan device lacks enabled Win32 external-memory/semaphore extensions."); return false; }
    VkPhysicalDeviceIDProperties id{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; properties.pNext = &id;
    vkGetPhysicalDeviceProperties2(instance.physicalDevice, &properties);
    if (!id.deviceLUIDValid) { log("Unity Vulkan adapter supplied no Windows LUID."); return false; }
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
    import_semaphore = reinterpret_cast<PFN_vkImportSemaphoreWin32HandleKHR>(vkGetDeviceProcAddr(instance.device, "vkImportSemaphoreWin32HandleKHR"));
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
    initialized = true; log("Unity Vulkan textures share same-adapter D3D12 resources and GPU fences."); return true;
}
bool begin(Slot& slot)
{
    if (vkWaitForFences(instance.device, 1, &slot.complete, VK_TRUE, 10000000000ull) != VK_SUCCESS) return false;
    for (auto command : slot.vk) {
        if (vkResetCommandBuffer(command, 0) != VK_SUCCESS) return false;
        VkCommandBufferBeginInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        if (vkBeginCommandBuffer(command, &info) != VK_SUCCESS) return false;
    }
    return SUCCEEDED(slot.allocator->Reset()) && SUCCEEDED(slot.commands->Reset(slot.allocator.Get(), nullptr));
}
void image_barrier(VkCommandBuffer commands, VkImage image, VkImageLayout before, VkImageLayout after,
    uint32_t from, uint32_t to, VkAccessFlags source, VkAccessFlags target)
{
    VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER}; barrier.srcAccessMask = source; barrier.dstAccessMask = target;
    barrier.oldLayout = before; barrier.newLayout = after; barrier.srcQueueFamilyIndex = from; barrier.dstQueueFamilyIndex = to;
    barrier.image = image; barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    vkCmdPipelineBarrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}
void acquire_image(VkCommandBuffer commands, Image& image, bool output)
{
    image_barrier(commands, image.image, image.initialized ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED,
        output ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL : VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_QUEUE_FAMILY_EXTERNAL, instance.queueFamilyIndex, 0, output ? VK_ACCESS_TRANSFER_READ_BIT : VK_ACCESS_TRANSFER_WRITE_BIT);
    image.initialized = true;
}
void release_image(VkCommandBuffer commands, Image& image, bool output)
{
    image_barrier(commands, image.image, output ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL : VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_GENERAL, instance.queueFamilyIndex, VK_QUEUE_FAMILY_EXTERNAL,
        output ? VK_ACCESS_TRANSFER_READ_BIT : VK_ACCESS_TRANSFER_WRITE_BIT, 0);
}
bool upload(Slot& slot, const VkSemaphore* waits = nullptr, uint32_t wait_count = 0)
{
    if (vkEndCommandBuffer(slot.vk[0]) != VK_SUCCESS) return false;
    std::vector<VkPipelineStageFlags> stages(wait_count, VK_PIPELINE_STAGE_TRANSFER_BIT);
    std::vector<uint64_t> values(wait_count, 0);
    const uint64_t value = ++fence_value;
    VkD3D12FenceSubmitInfoKHR fence{VK_STRUCTURE_TYPE_D3D12_FENCE_SUBMIT_INFO_KHR};
    fence.waitSemaphoreValuesCount = wait_count; fence.pWaitSemaphoreValues = values.data();
    fence.signalSemaphoreValuesCount = 1; fence.pSignalSemaphoreValues = &value;
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.pNext = &fence;
    submit.waitSemaphoreCount = wait_count; submit.pWaitSemaphores = waits; submit.pWaitDstStageMask = stages.data();
    submit.commandBufferCount = 1; submit.pCommandBuffers = &slot.vk[0]; submit.signalSemaphoreCount = 1; submit.pSignalSemaphores = &semaphore;
    return vkQueueSubmit(instance.graphicsQueue, 1, &submit, VK_NULL_HANDLE) == VK_SUCCESS && SUCCEEDED(queue->Wait(shared_fence.Get(), value));
}
bool submit(Slot& slot, VkSemaphore ready = VK_NULL_HANDLE)
{
    if (FAILED(slot.commands->Close()) || vkEndCommandBuffer(slot.vk[1]) != VK_SUCCESS) return false;
    ID3D12CommandList* lists[] = {slot.commands.Get()}; queue->ExecuteCommandLists(1, lists);
    const uint64_t value = ++fence_value;
    if (FAILED(queue->Signal(shared_fence.Get(), value))) return false;
    VkD3D12FenceSubmitInfoKHR fence{VK_STRUCTURE_TYPE_D3D12_FENCE_SUBMIT_INFO_KHR};
    fence.waitSemaphoreValuesCount = 1; fence.pWaitSemaphoreValues = &value;
    uint64_t zero = 0; fence.signalSemaphoreValuesCount = ready ? 1u : 0u; fence.pSignalSemaphoreValues = &zero;
    VkPipelineStageFlags stage = VK_PIPELINE_STAGE_TRANSFER_BIT;
    VkSubmitInfo submit_info{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit_info.pNext = &fence;
    submit_info.waitSemaphoreCount = 1; submit_info.pWaitSemaphores = &semaphore; submit_info.pWaitDstStageMask = &stage;
    submit_info.commandBufferCount = 1; submit_info.pCommandBuffers = &slot.vk[1];
    submit_info.signalSemaphoreCount = ready ? 1u : 0u; submit_info.pSignalSemaphores = &ready;
    if (vkResetFences(instance.device, 1, &slot.complete) != VK_SUCCESS) return false;
    return vkQueueSubmit(instance.graphicsQueue, 1, &submit_info, slot.complete) == VK_SUCCESS;
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
struct Event {
    rsf_unity_packet packet{};
    std::array<UnityVulkanImage, 4> textures{};
    bool reconfigure = false, hudless = false, present = false;
};
void UNITY_INTERFACE_API execute_queue(int, void* data) noexcept try
{
    std::unique_ptr<Event> event(static_cast<Event*>(data));
    struct Finish { ~Finish() { callbacks.fetch_sub(1); } } finish;
    if (faulted) return;
    if (!initialize()) {
        faulted = true;
        rsf_unity_package_bridge_error(RSF_BACKEND_ERROR_INIT_FAILED, "Unity Vulkan shared device/fence initialization failed; native output remains in place.");
        return;
    }
    auto& packet = event->packet;
    auto& slot = slots[next_slot]; if (!begin(slot)) { faulted = true; return; }
    if (event->reconfigure && !drain()) { faulted = true; return; }
    constexpr VkFormat formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
    constexpr DXGI_FORMAT dx_formats[] = {DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT};
    const bool probe = (packet.flags & RSF_UNITY_PACKET_PROBE) != 0;
    if (event->hudless || event->present) {
        const auto& texture = event->textures[0];
        const uint32_t target = event->present ? 5u : 4u;
        if (!create_image(target, packet.camera.output_width, packet.camera.output_height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM)) { faulted = true; return; }
        acquire_image(slot.vk[0], images[target], false);
        copy(slot.vk[0], texture.image, texture.layout, texture.aspect, images[target].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, images[target].width, images[target].height);
        release_image(slot.vk[0], images[target], false);
    } else for (uint32_t i = 0; i < 4; ++i) {
        if (probe && (i == 1 || i == 2)) continue;
        uint32_t width = i == 3 ? packet.camera.output_width : packet.camera.render_width;
        uint32_t height = i == 3 ? packet.camera.output_height : packet.camera.render_height;
        if (!create_image(i, width, height, formats[i], dx_formats[i])) { faulted = true; return; }
        acquire_image(slot.vk[0], images[i], false);
        copy(slot.vk[0], event->textures[i].image, event->textures[i].layout, event->textures[i].aspect,
            images[i].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, width, height);
        release_image(slot.vk[0], images[i], false);
    }
    if (!upload(slot)) { faulted = true; return; }
    rsf_game_render_pass pass{}; make_pass(packet, pass);
    if (event->reconfigure) { pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0; }
    if (event->present) {
        pass.role = RSF_GAME_RENDER_WINDOW; pass.color_input = images[5].resource.Get(); pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
        host.render_pass(host.user, queue.Get(), &pass, 1);
    } else if (event->hudless) {
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
        if (!probe && !(event->reconfigure)) {
            acquire_image(slot.vk[1], images[3], true);
            copy(slot.vk[1], images[3].image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                event->textures[3].image, event->textures[3].layout, images[3].width, images[3].height);
            release_image(slot.vk[1], images[3], true);
        }
    }
    if (!submit(slot)) { faulted = true; return; }
    host.render_pass(host.user, slot.commands.Get(), &pass, 0);
    next_slot = (next_slot + 1) % 3;
} catch (...) { faulted = true; log("Unity Vulkan queue execution failed; resources remain owned."); }

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
bool configure_chain(uint32_t width, uint32_t height)
{
    if (!window) return false;
    if (!chain) {
        ComPtr<IDXGIFactory2> factory; ComPtr<IDXGISwapChain1> created;
        DXGI_SWAP_CHAIN_DESC1 desc{}; desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->CreateSwapChainForHwnd(queue.Get(), window, &desc, nullptr, nullptr, &created)) ||
            FAILED(created.As(&chain))) return false;
    } else {
        DXGI_SWAP_CHAIN_DESC1 desc{}; chain->GetDesc1(&desc);
        if ((desc.Width != width || desc.Height != height) &&
            (!drain() || FAILED(chain->ResizeBuffers(3, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0)))) return false;
    }
    return true;
}
VKAPI_ATTR VkResult VKAPI_CALL intercept_present(VkQueue vk_queue, const VkPresentInfoKHR* info) noexcept try
{
    if (!original_present || !info) return VK_ERROR_INITIALIZATION_FAILED;
    if (!window || faulted || !last_window.frame_id || info->swapchainCount != 1 || !initialize()) return original_present(vk_queue, info);
    std::lock_guard<std::mutex> lock(swapchain_guard);
    auto found = swapchains.find(info->pSwapchains[0]);
    if (found == swapchains.end()) return original_present(vk_queue, info);
    auto& source = found->second; const uint32_t index = info->pImageIndices[0];
    if (index >= source.images.size() || !configure_chain(source.extent.width, source.extent.height) ||
        !create_image(5, source.extent.width, source.extent.height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM)) return original_present(vk_queue, info);
    auto& slot = slots[next_slot]; if (!begin(slot)) return original_present(vk_queue, info);
    acquire_image(slot.vk[0], images[5], false);
    image_barrier(slot.vk[0], source.images[index], VK_IMAGE_LAYOUT_PRESENT_SRC_KHR, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, 0, VK_ACCESS_TRANSFER_READ_BIT);
    VkImageBlit blit{}; blit.srcSubresource = blit.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    blit.srcOffsets[1] = blit.dstOffsets[1] = {static_cast<int32_t>(source.extent.width), static_cast<int32_t>(source.extent.height), 1};
    vkCmdBlitImage(slot.vk[0], source.images[index], VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, images[5].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &blit, VK_FILTER_NEAREST);
    image_barrier(slot.vk[0], source.images[index], VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, VK_ACCESS_TRANSFER_READ_BIT, 0);
    release_image(slot.vk[0], images[5], false);
    if (!upload(slot, info->pWaitSemaphores, info->waitSemaphoreCount)) { faulted = true; return VK_ERROR_DEVICE_LOST; }
    ComPtr<ID3D12Resource> buffer;
    if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&buffer)))) { faulted = true; return VK_ERROR_DEVICE_LOST; }
    dx_barrier(slot.commands.Get(), images[5].resource.Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE);
    dx_barrier(slot.commands.Get(), buffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    slot.commands->CopyResource(buffer.Get(), images[5].resource.Get());
    dx_barrier(slot.commands.Get(), buffer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    dx_barrier(slot.commands.Get(), images[5].resource.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
    rsf_game_render_pass pass{}; make_pass(last_window, pass); pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = chain.Get();
    host.render_pass(host.user, queue.Get(), &pass, 1);
    if (!submit(slot, source.present_ready[index])) { faulted = true; return VK_ERROR_DEVICE_LOST; }
    const HRESULT presented = chain->Present(0, 0);
    if (SUCCEEDED(presented)) ShowWindowAsync(window, SW_SHOWNOACTIVATE);
    auto forwarded = *info; forwarded.waitSemaphoreCount = 1; forwarded.pWaitSemaphores = &source.present_ready[index];
    next_slot = (next_slot + 1) % 3;
    return original_present(vk_queue, &forwarded);
} catch (...) { faulted = true; return VK_ERROR_DEVICE_LOST; }
VKAPI_ATTR VkResult VKAPI_CALL intercept_create_device(VkPhysicalDevice physical, const VkDeviceCreateInfo* info, const VkAllocationCallbacks* allocator, VkDevice* output)
{
    if (!original_create_device) return VK_ERROR_INITIALIZATION_FAILED;
    uint32_t count = 0; vkEnumerateDeviceExtensionProperties(physical, nullptr, &count, nullptr);
    std::vector<VkExtensionProperties> available(count); vkEnumerateDeviceExtensionProperties(physical, nullptr, &count, available.data());
    std::vector<const char*> extensions(info->ppEnabledExtensionNames, info->ppEnabledExtensionNames + info->enabledExtensionCount);
    for (const char* name : {VK_KHR_EXTERNAL_MEMORY_WIN32_EXTENSION_NAME, VK_KHR_EXTERNAL_SEMAPHORE_WIN32_EXTENSION_NAME}) {
        if (std::none_of(available.begin(), available.end(), [name](const auto& item) { return std::strcmp(item.extensionName, name) == 0; }))
            return original_create_device(physical, info, allocator, output);
        if (std::none_of(extensions.begin(), extensions.end(), [name](const char* item) { return std::strcmp(item, name) == 0; })) extensions.push_back(name);
    }
    auto request = *info; request.enabledExtensionCount = static_cast<uint32_t>(extensions.size()); request.ppEnabledExtensionNames = extensions.data();
    const auto result = original_create_device(physical, &request, allocator, output);
    external_extensions = result == VK_SUCCESS; return result;
}
VKAPI_ATTR VkResult VKAPI_CALL intercept_create_swapchain(VkDevice vk_device, const VkSwapchainCreateInfoKHR* info, const VkAllocationCallbacks* allocator, VkSwapchainKHR* output)
{
    auto request = *info; request.imageUsage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    const auto result = original_create_swapchain(vk_device, &request, allocator, output);
    if (result != VK_SUCCESS) return result;
    Swapchain source; source.device = vk_device; source.extent = request.imageExtent; source.format = request.imageFormat;
    uint32_t count = 0; vkGetSwapchainImagesKHR(vk_device, *output, &count, nullptr);
    source.images.resize(count); source.present_ready.resize(count);
    vkGetSwapchainImagesKHR(vk_device, *output, &count, source.images.data());
    VkSemaphoreCreateInfo semaphore_info{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    for (auto& ready : source.present_ready) if (vkCreateSemaphore(vk_device, &semaphore_info, nullptr, &ready) != VK_SUCCESS) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    std::lock_guard<std::mutex> lock(swapchain_guard); swapchains[*output] = std::move(source); return result;
}
VKAPI_ATTR void VKAPI_CALL intercept_destroy_swapchain(VkDevice vk_device, VkSwapchainKHR swapchain, const VkAllocationCallbacks* allocator)
{
    { std::lock_guard<std::mutex> lock(swapchain_guard);
      auto found = swapchains.find(swapchain);
      if (found != swapchains.end()) {
          vkDeviceWaitIdle(vk_device);
          for (auto ready : found->second.present_ready) vkDestroySemaphore(vk_device, ready, nullptr);
          swapchains.erase(found);
      } }
    original_destroy_swapchain(vk_device, swapchain, allocator);
}
PFN_vkVoidFunction intercept(const char* name, PFN_vkVoidFunction original)
{
    if (std::strcmp(name, "vkCreateDevice") == 0) { original_create_device = reinterpret_cast<PFN_vkCreateDevice>(original); return reinterpret_cast<PFN_vkVoidFunction>(intercept_create_device); }
    if (std::strcmp(name, "vkCreateSwapchainKHR") == 0) { original_create_swapchain = reinterpret_cast<PFN_vkCreateSwapchainKHR>(original); return reinterpret_cast<PFN_vkVoidFunction>(intercept_create_swapchain); }
    if (std::strcmp(name, "vkDestroySwapchainKHR") == 0) { original_destroy_swapchain = reinterpret_cast<PFN_vkDestroySwapchainKHR>(original); return reinterpret_cast<PFN_vkVoidFunction>(intercept_destroy_swapchain); }
    if (std::strcmp(name, "vkQueuePresentKHR") == 0) { original_present = reinterpret_cast<PFN_vkQueuePresentKHR>(original); return reinterpret_cast<PFN_vkVoidFunction>(intercept_present); }
    return original;
}
VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL get_device_proc(VkDevice vk_device, const char* name) { return intercept(name, original_gdpa(vk_device, name)); }
VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL get_instance_proc(VkInstance vk_instance, const char* name)
{
    auto original = original_gipa(vk_instance, name);
    if (std::strcmp(name, "vkGetDeviceProcAddr") == 0) { original_gdpa = reinterpret_cast<PFN_vkGetDeviceProcAddr>(original); return reinterpret_cast<PFN_vkVoidFunction>(get_device_proc); }
    return intercept(name, original);
}
PFN_vkGetInstanceProcAddr UNITY_INTERFACE_API initialize_intercept(PFN_vkGetInstanceProcAddr original, void*) { original_gipa = original; return get_instance_proc; }
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
    instance = unity->Instance();
    UnityVulkanPluginEventConfig config{}; config.renderPassPrecondition = kUnityVulkanRenderPass_EnsureOutside;
    config.graphicsQueueAccess = kUnityVulkanGraphicsQueueAccess_DontCare;
    config.flags = kUnityVulkanEventConfigFlag_SyncWorkerThreads;
    for (int offset = 0; offset < 3; ++offset) unity->ConfigureEvent(base_event + offset, &config);
}
bool rsf_uvk_create_window()
{
    if (window) return true;
    HWND parent = nullptr; EnumWindows(find_window, reinterpret_cast<LPARAM>(&parent));
    if (!parent || GetWindowThreadProcessId(parent, nullptr) != GetCurrentThreadId()) return false;
    WNDCLASSW type{}; type.lpfnWndProc = window_proc; type.hInstance = GetModuleHandleW(nullptr); type.lpszClassName = L"ReScaleFrame.Unity.Presentation";
    if (!RegisterClassW(&type) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return false;
    RECT client{}; GetClientRect(parent, &client);
    window = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, type.lpszClassName, L"", WS_CHILD | WS_CLIPSIBLINGS,
        0, 0, client.right, client.bottom, parent, nullptr, type.hInstance, nullptr);
    return window && rsf_uvk_update_window(static_cast<uint32_t>(client.right), static_cast<uint32_t>(client.bottom));
}
bool rsf_uvk_owns_window(void* candidate) { return window && candidate == window; }
bool rsf_uvk_graphics(void** owning_device, void** owning_queue)
{
    if (!initialized || !owning_device || !owning_queue) return false;
    *owning_device = device.Get(); *owning_queue = queue.Get(); return true;
}
bool rsf_uvk_update_window(uint32_t width, uint32_t height)
{
    if (!window || !width || !height || GetWindowThreadProcessId(window, nullptr) != GetCurrentThreadId()) return false;
    RECT client{}; if (!GetClientRect(window, &client)) return false;
    if ((client.right != static_cast<LONG>(width) || client.bottom != static_cast<LONG>(height)) &&
        !SetWindowPos(window, nullptr, 0, 0, static_cast<int>(width), static_cast<int>(height), SWP_NOACTIVATE | SWP_NOZORDER)) return false;
    // Establish the shared SDK owner before SR can register a standalone instance. Initial
    // DXGI creation runs on the window thread; subsequent GPU-drained resize stays in Present.
    return chain || (initialize() && configure_chain(width, height));
}
bool rsf_uvk_render(int event_id, const rsf_unity_packet& packet, const rsf_game_host_services& services, bool reconfigure)
{
    if (!unity || faulted) return false;
    host = services;
    if (event_id == base_event + 1 && !rsf_unity_package_is_editor()) { last_window = packet; return true; }
    auto event = std::make_unique<Event>(); event->packet = packet; event->reconfigure = reconfigure; event->hudless = event_id == base_event + 2;
    event->present = event_id == base_event + 1;
    void* pointers[] = {packet.color, packet.depth, packet.motion, packet.output};
    for (uint32_t i = 0; i < 4; ++i) {
        if (!pointers[i]) continue;
        const bool output = i == 3;
        if (!unity->AccessTexture(pointers[i], UnityVulkanWholeImage,
            output ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            VK_PIPELINE_STAGE_TRANSFER_BIT, output ? (VK_ACCESS_TRANSFER_WRITE_BIT | VK_ACCESS_TRANSFER_READ_BIT) : VK_ACCESS_TRANSFER_READ_BIT,
            kUnityVulkanResourceAccess_PipelineBarrier, &event->textures[i])) { log("Unity Vulkan texture access was refused."); return false; }
    }
    if (!event->textures[0].image) return false;
    callbacks.fetch_add(1); unity->AccessQueue(execute_queue, event_id, event.release(), true);
    return true;
}
bool rsf_uvk_drained()
{
    if (callbacks.load()) return false;
    for (auto& slot : slots) if (slot.complete && vkGetFenceStatus(instance.device, slot.complete) != VK_SUCCESS) return false;
    return true;
}
void rsf_uvk_release()
{
    if (!rsf_uvk_drained() || !drain()) return;
    for (auto& image : images) destroy_image(image);
    for (auto& slot : slots) { if (slot.complete) vkDestroyFence(instance.device, slot.complete, nullptr); slot = {}; }
    if (command_pool) vkDestroyCommandPool(instance.device, command_pool, nullptr);
    if (semaphore) vkDestroySemaphore(instance.device, semaphore, nullptr);
    command_pool = VK_NULL_HANDLE; semaphore = VK_NULL_HANDLE; chain.Reset(); shared_fence.Reset(); queue.Reset(); device.Reset();
    next_slot = 0; fence_value = 0; last_window = {}; faulted = initialized = false;
}
