// SPDX-License-Identifier: MIT
// Unity's versioned graphics dispatch and the exception guards at the Rust boundary.
#include "vulkan_bridge.h"
#include "vulkan_foreign.h"
#include "vulkan_state.h"
#include <IUnityGraphics.h>
#include <IUnityGraphicsVulkan.h>
#include <bit>
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
static_assert(sizeof(VulkanHooks) == 8 * sizeof(void*));
static_assert(sizeof(VkExtensionProperties) == 260 && offsetof(VkExtensionProperties, specVersion) == 256);
static_assert(sizeof(VkSwapchainCreateInfoKHR) == 104 && offsetof(VkSwapchainCreateInfoKHR, imageUsage) == 56 &&
    offsetof(VkSwapchainCreateInfoKHR, oldSwapchain) == 96);
static_assert(sizeof(VkWin32SurfaceCreateInfoKHR) == 40 && offsetof(VkWin32SurfaceCreateInfoKHR, hwnd) == 32);
static_assert(sizeof(VkSemaphoreCreateInfo) == 24);
static_assert(sizeof(VkSemaphore) == sizeof(uint64_t));
static_assert(sizeof(VkDeviceCreateInfo) == 72 && offsetof(VkDeviceCreateInfo, enabledExtensionCount) == 48 &&
    offsetof(VkDeviceCreateInfo, ppEnabledExtensionNames) == 56);

static_assert(sizeof(rsf_uvk_engine_instance) == 40 && offsetof(rsf_uvk_engine_instance, queue_family) == 32);
static_assert(sizeof(rsf_uvk_engine_image) == 16 && offsetof(rsf_uvk_engine_image, layout) == 8);
static_assert(sizeof(VkPresentInfoKHR) == 64 && offsetof(VkPresentInfoKHR, pSwapchains) == 40);
static_assert(sizeof(rsf_unity_vulkan_device_handles) == 72 && offsetof(rsf_unity_vulkan_device_handles, dispatch) == 64);

namespace {
IUnityGraphicsVulkanV2* unity = nullptr;
int base_event = 0;

void UNITY_INTERFACE_API execute_queue(int, void* event) noexcept
{
    rsf_uvk_bridge_execute(event);
}

VKAPI_ATTR VkResult VKAPI_CALL intercept_present(VkQueue queue, const VkPresentInfoKHR* info) noexcept
{
    return static_cast<VkResult>(rsf_uvk_bridge_present(reinterpret_cast<uint64_t>(queue), info));
}

VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL instance_lookup(VkInstance instance, const char* name) noexcept try
{
    return vkGetInstanceProcAddr(instance, name);
} catch (...) { return nullptr; }

VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL device_lookup(VkDevice device, const char* name) noexcept try
{
    return vkGetDeviceProcAddr(device, name);
} catch (...) { return nullptr; }

PFN_vkGetInstanceProcAddr UNITY_INTERFACE_API initialize_intercept(PFN_vkGetInstanceProcAddr original, void*) noexcept
{
    const VulkanHooks hooks{intercept_present, device_lookup, vkEnumerateDeviceExtensionProperties,
        vkCreateDevice, vkCreateSemaphore, vkDestroySemaphore, vkDeviceWaitIdle, rsf_uvk_bridge_retirement_fault};
    return rsf_unity_vk_install(original, &hooks);
}
}

extern "C" bool rsf_uvk_foreign_available(void) { return unity != nullptr; }

extern "C" bool rsf_uvk_foreign_instance(rsf_uvk_engine_instance* output) try
{
    if (!unity) return false;
    const auto instance = unity->Instance();
    *output = {reinterpret_cast<uint64_t>(instance.instance), reinterpret_cast<uint64_t>(instance.device),
        reinterpret_cast<uint64_t>(instance.physicalDevice), reinterpret_cast<uint64_t>(instance.graphicsQueue),
        instance.queueFamilyIndex};
    return true;
} catch (...) { return false; }

extern "C" bool rsf_uvk_foreign_create_device(const rsf_game_host_services* services, rsf_unity_vulkan_device_handles* output)
{
    rsf_uvk_engine_instance instance{};
    return rsf_uvk_foreign_instance(&instance) && rsf_uvk_bridge_create_device(services, &instance, output,
        std::bit_cast<void (*)(void)>(&instance_lookup), std::bit_cast<void (*)(void)>(&device_lookup));
}

extern "C" bool rsf_uvk_foreign_texture(void* texture, int32_t layout, uint32_t access, rsf_uvk_engine_image* output) try
{
    UnityVulkanImage image{};
    if (!unity->AccessTexture(texture, UnityVulkanWholeImage, static_cast<VkImageLayout>(layout),
        VK_PIPELINE_STAGE_TRANSFER_BIT, access, kUnityVulkanResourceAccess_PipelineBarrier, &image)) return false;
    *output = {reinterpret_cast<uint64_t>(image.image), static_cast<int32_t>(image.layout), image.aspect};
    return true;
} catch (...) { return false; }

extern "C" bool rsf_uvk_foreign_queue(int event_id, void* event) try
{
    unity->AccessQueue(execute_queue, event_id, event, true);
    return true;
} catch (...) { return false; }

extern "C" void rsf_uvk_foreign_log(const rsf_game_host_services* services, const char* text) try
{
    services->log(services->user, text);
} catch (...) {}

extern "C" bool rsf_uvk_foreign_pass(const rsf_game_host_services* services, void* target,
    const rsf_game_render_pass* pass, uint32_t phase) try
{
    services->render_pass(services->user, target, pass, phase);
    return true;
} catch (...) { return false; }

extern "C" bool rsf_uvk_foreign_present(uint64_t queue, const void* info, int32_t* result) try
{
    // The loader export dispatches arbitrary queues, including those outside this bridge.
    *result = vkQueuePresentKHR(reinterpret_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
    return true;
} catch (...) { return false; }

void rsf_uvk_preload(IUnityInterfaces* interfaces) try
{
    unity = interfaces->Get<IUnityGraphicsVulkanV2>();
    if (unity) (void)unity->AddInterceptInitialization(initialize_intercept, nullptr, 0);
} catch (...) { unity = nullptr; }

void rsf_uvk_load(IUnityInterfaces* interfaces, int event_id) try
{
    unity = interfaces->Get<IUnityGraphicsVulkanV2>();
    base_event = event_id;
    if (!unity) return;
    UnityVulkanPluginEventConfig config{};
    config.renderPassPrecondition = kUnityVulkanRenderPass_EnsureOutside;
    config.graphicsQueueAccess = kUnityVulkanGraphicsQueueAccess_DontCare;
    config.flags = kUnityVulkanEventConfigFlag_SyncWorkerThreads;
    for (int offset = 0; offset < 3; ++offset) unity->ConfigureEvent(base_event + offset, &config);
} catch (...) { unity = nullptr; }

void rsf_uvk_unload() noexcept { unity = nullptr; }

bool rsf_uvk_create_window() { return rsf_uvk_bridge_create_window(); }
bool rsf_uvk_owns_window(void* candidate) { return rsf_uvk_bridge_owns_window(candidate); }
bool rsf_uvk_update_window(uint32_t width, uint32_t height) { return rsf_uvk_bridge_update_window(width, height); }

bool rsf_uvk_graphics(Microsoft::WRL::ComPtr<ID3D12Device>& device,
    Microsoft::WRL::ComPtr<ID3D12CommandQueue>& queue) noexcept
{
    void* owned_device = nullptr;
    void* owned_queue = nullptr;
    if (!rsf_uvk_bridge_graphics(&owned_device, &owned_queue)) return false;
    device.Attach(static_cast<ID3D12Device*>(owned_device));
    queue.Attach(static_cast<ID3D12CommandQueue*>(owned_queue));
    return true;
}

bool rsf_uvk_render(int event_id, const rsf_unity_packet& packet, const rsf_game_host_services& services, bool reconfigure)
{
    return rsf_uvk_bridge_render(event_id, base_event, &packet, &services, reconfigure);
}
