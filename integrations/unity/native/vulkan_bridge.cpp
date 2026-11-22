// SPDX-License-Identifier: MIT
// Unity SDK access, foreign callbacks and player-window presentation for the Vulkan route.
// Rust owns the shared device and command execution. This boundary lends Unity's textures and
// keeps the queue callback and intercepted Present under the Rust admission and presentation holds.
#include "vulkan_bridge.h"
#include "vulkan_state.h"
#include "vulkan_gpu.h"
#include "bridge_interop.h"
#include "bridge_state.h"
#include "package.h"
#include <windows.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsVulkan.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <array>
#include <cstddef>

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
static_assert(sizeof(VkSemaphore) == sizeof(uint64_t));
static_assert(sizeof(VkDeviceCreateInfo) == 72 && offsetof(VkDeviceCreateInfo, enabledExtensionCount) == 48 &&
    offsetof(VkDeviceCreateInfo, ppEnabledExtensionNames) == 56);

// Private package receipt ABI shared with the D3D11 and D3D12 producers.
extern "C" uint64_t rsf_unity_package_sr_ring_begin(void);
extern "C" void rsf_unity_package_sr_ring_retired(uint64_t epoch);

static_assert(sizeof(rsf_unity_vulkan_present_source) == 24, "vulkan_state.rs PresentSource");
static_assert(sizeof(rsf_unity_vulkan_image_handles) == 24);
static_assert(sizeof(rsf_unity_vulkan_device_handles) == 72);
static_assert(offsetof(rsf_unity_vulkan_device_handles, vk_device) == 24);
static_assert(offsetof(rsf_unity_vulkan_device_handles, vk_queue_family) == 56);
static_assert(offsetof(rsf_unity_vulkan_device_handles, dispatch) == 64);
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
    VkDevice vk_device() const { return from_id<VkDevice>(handles.vk_device); }
    VkQueue vk_queue() const { return from_id<VkQueue>(handles.vk_queue); }
};
// Only a view. Rust retains the imported image and D3D12 resource for the GPU lifetime.
struct ImageView {
    rsf_unity_vulkan_image_handles handles{};
    uint64_t image() const { return handles.image; }
    ID3D12Resource* resource() const { return static_cast<ID3D12Resource*>(handles.resource); }
};
struct SlotView {
    rsf_unity_vulkan_command_handles handles{};
    ID3D12GraphicsCommandList* commands() const { return static_cast<ID3D12GraphicsCommandList*>(handles.commands); }
    uint64_t vk(std::size_t index) const { return handles.vk[index]; }
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
bool create_image(uint32_t index, uint32_t width, uint32_t height, VkFormat format,
    DXGI_FORMAT dx_format, ImageView& view)
{
    return rsf_unity_vulkan_prepare_image(index, width, height, static_cast<uint32_t>(format),
        static_cast<uint32_t>(dx_format), &view.handles);
}
bool create_shared(const rsf_game_host_services* services, rsf_unity_vulkan_device_handles* output) noexcept try
{
    const auto instance = unity->Instance();
    if (!instance.instance || !instance.device || !instance.physicalDevice || !instance.graphicsQueue) return false;
    output->vk_device = handle_id(instance.device);
    output->vk_physical_device = handle_id(instance.physicalDevice);
    output->vk_queue = handle_id(instance.graphicsQueue);
    output->vk_queue_family = instance.queueFamilyIndex;
    if (!rsf_unity_vk_interop_ready(instance.device, vkGetDeviceProcAddr)) {
        log(services, "Unity Vulkan device lacks enabled Win32 external-memory/semaphore extensions.");
        return false;
    }
    const auto created = rsf_uvk_gpu_create_device(output, handle_id(instance.instance),
        vkGetInstanceProcAddr, vkGetDeviceProcAddr);
    if (created == 2) log(services, "Unity Vulkan adapter supplied no Windows LUID.");
    return created == 1;
} catch (...) { return false; }
void report_created(const rsf_game_host_services* services) noexcept try
{
    log(services, "Unity Vulkan textures share same-adapter D3D12 resources and GPU fences.");
} catch (...) {}
constexpr rsf_unity_vulkan_device_ops device_ops{
    create_shared, rsf_uvk_gpu_destroy_device, rsf_uvk_gpu_create_pool, rsf_uvk_gpu_destroy_pool,
    rsf_uvk_gpu_create_slot, rsf_uvk_gpu_destroy_slot, rsf_uvk_gpu_create_image, rsf_uvk_gpu_destroy_image,
    rsf_uvk_gpu_fence_ready, rsf_uvk_gpu_shared_completed, rsf_uvk_gpu_drain,
    report_created, rsf_unity_package_sr_ring_begin, rsf_unity_package_sr_ring_retired,
};
uint32_t initialize(DeviceView& graphics, const rsf_game_host_services* services = nullptr)
{
    return unity ? rsf_unity_vulkan_initialize(services, &device_ops, &graphics.handles) : RSF_UNITY_VULKAN_DEVICE_FAILED;
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
    if (!rsf_unity_vulkan_command_slot(ring, &slot.handles) || !rsf_uvk_gpu_begin(&graphics.handles, &slot.handles, ring)) {
        stop("Unity Vulkan command slot could not be reused; the bridge stopped.");
        return;
    }
    if (reconfigure && !rsf_uvk_gpu_drain(&graphics.handles)) { stop("Unity Vulkan GPU drain before reconfiguration failed; the bridge stopped."); return; }
    constexpr VkFormat formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
    constexpr DXGI_FORMAT dx_formats[] = {DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT};
    const bool probe = (packet.flags & RSF_UNITY_PACKET_PROBE) != 0;
    if (hudless || present) {
        const auto& texture = textures[0];
        const uint32_t target = present ? 5u : 4u;
        if (!create_image(target, packet.camera.output_width, packet.camera.output_height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, images[target])) { stop("Unity Vulkan shared image creation failed; the bridge stopped."); return; }
        rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(0), images[target].image(), target, false, true);
        rsf_uvk_gpu_copy(&graphics.handles, slot.vk(0), handle_id(texture.image), texture.layout, texture.aspect, images[target].image(), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, packet.camera.output_width, packet.camera.output_height);
        rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(0), images[target].image(), 0, false, false);
    } else for (uint32_t i = 0; i < 4; ++i) {
        if (probe && (i == 1 || i == 2)) continue;
        uint32_t width = i == 3 ? packet.camera.output_width : packet.camera.render_width;
        uint32_t height = i == 3 ? packet.camera.output_height : packet.camera.render_height;
        if (!create_image(i, width, height, formats[i], dx_formats[i], images[i])) { stop("Unity Vulkan shared image creation failed; the bridge stopped."); return; }
        rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(0), images[i].image(), i, false, true);
        rsf_uvk_gpu_copy(&graphics.handles, slot.vk(0), handle_id(textures[i].image), textures[i].layout, textures[i].aspect,
            images[i].image(), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, width, height);
        rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(0), images[i].image(), 0, false, false);
    }
    if (!rsf_uvk_gpu_upload(&graphics.handles, &slot.handles, nullptr, 0)) { stop("Unity Vulkan upload submission failed; the bridge stopped."); return; }
    rsf_game_render_pass pass{}; rsf_unity_build_scalar_pass(&packet, &pass);
    if (reconfigure) { pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0; }
    if (present) {
        pass.role = RSF_GAME_RENDER_WINDOW; pass.color_input = images[5].resource(); pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
        host.render_pass(host.user, graphics.queue(), &pass, 1);
    } else if (hudless) {
        pass.role = RSF_GAME_RENDER_FINAL_SCENE; pass.color_input = images[4].resource();
        rsf_uvk_gpu_dx_barrier(slot.commands(), images[4].resource(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
        host.render_pass(host.user, slot.commands(), &pass, 1);
        rsf_uvk_gpu_dx_barrier(slot.commands(), images[4].resource(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COMMON);
    } else {
        void** pointers[] = {&pass.color_input, &pass.depth, &pass.motion, &pass.color_output};
        for (uint32_t i = 0; i < 4; ++i) if (!(probe && (i == 1 || i == 2))) {
            *pointers[i] = images[i].resource();
            rsf_uvk_gpu_dx_barrier(slot.commands(), images[i].resource(), D3D12_RESOURCE_STATE_COMMON,
                i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        }
        host.render_pass(host.user, slot.commands(), &pass, 1);
        for (uint32_t i = 0; i < 4; ++i) if (!(probe && (i == 1 || i == 2)))
            rsf_uvk_gpu_dx_barrier(slot.commands(), images[i].resource(), i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
        if (!probe && !reconfigure) {
            rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(1), images[3].image(), 3, true, true);
            rsf_uvk_gpu_copy(&graphics.handles, slot.vk(1), images[3].image(), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                handle_id(textures[3].image), textures[3].layout, packet.camera.output_width, packet.camera.output_height);
            rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(1), images[3].image(), 0, true, false);
        }
    }
    if (!rsf_uvk_gpu_submit(&graphics.handles, &slot.handles, ring, 0)) { stop("Unity Vulkan render submission failed; the bridge stopped."); return; }
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
constexpr rsf_unity_vulkan_surface_ops surface_ops{
    rsf_uvk_gpu_chain_create, rsf_uvk_gpu_chain_extent, rsf_uvk_gpu_drain,
    rsf_uvk_gpu_chain_resize, rsf_uvk_gpu_chain_destroy,
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
    auto* chain = rsf_unity_vulkan_configure_surface(source.width, source.height, &surface_ops);
    if (!chain) return native();
    const VkSemaphore present_ready = from_id<VkSemaphore>(source.present_ready);
    ImageView image;
    if (!create_image(5, source.width, source.height, VK_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, image)) return native();
    // Preflight before the upload consumes Unity's present waits.
    ComPtr<ID3D12Resource> buffer;
    buffer.Attach(static_cast<ID3D12Resource*>(rsf_uvk_gpu_chain_buffer(chain)));
    if (!buffer) {
        stop("Unity Vulkan swap chain buffer was unavailable; the bridge stopped.");
        return native();
    }
    const uint32_t ring = rsf_submission_acquire(rsf_unity_vulkan_submission_ring(), nullptr);
    SlotView slot;
    if (!rsf_unity_vulkan_command_slot(ring, &slot.handles) || !rsf_uvk_gpu_begin(&graphics.handles, &slot.handles, ring)) {
        stop("Unity Vulkan command slot could not be reused; the bridge stopped.");
        return native();
    }
    rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(0), image.image(), 5, false, true);
    rsf_uvk_gpu_blit_present(&graphics.handles, slot.vk(0), source.image, image.image(), source.width, source.height);
    rsf_uvk_gpu_image_transition(&graphics.handles, slot.vk(0), image.image(), 0, false, false);
    if (!rsf_uvk_gpu_upload(&graphics.handles, &slot.handles, info->pWaitSemaphores, info->waitSemaphoreCount)) { stop("Unity Vulkan present upload failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    rsf_uvk_gpu_dx_barrier(slot.commands(), image.resource(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE);
    rsf_uvk_gpu_dx_barrier(slot.commands(), buffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    rsf_uvk_gpu_dx_copy(slot.commands(), buffer.Get(), image.resource());
    rsf_uvk_gpu_dx_barrier(slot.commands(), buffer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    rsf_uvk_gpu_dx_barrier(slot.commands(), image.resource(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
    rsf_game_render_pass pass{}; rsf_unity_build_scalar_pass(&frame.packet, &pass); pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = chain;
    frame.services.render_pass(frame.services.user, graphics.queue(), &pass, 1);
    if (!rsf_uvk_gpu_submit(&graphics.handles, &slot.handles, ring, source.present_ready)) { stop("Unity Vulkan present submission failed; the bridge stopped."); return VK_ERROR_DEVICE_LOST; }
    if (rsf_uvk_gpu_chain_present(chain)) ShowWindowAsync(window, SW_SHOWNOACTIVATE);
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
