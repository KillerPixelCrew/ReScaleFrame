/* SPDX-License-Identifier: MIT */
/* Unity's versioned Vulkan interface, the loader hooks handed to the Rust resolver, and the
   exception guards around foreign calls at the Rust boundary (rescaleframe/foreign_guard.h:
   foreign C++ exceptions only; access violations and other faults still propagate). */
#include "vulkan_bridge.h"
#include "vulkan_foreign.h"
#include "vulkan_state.h"
#include <stddef.h>
#include <windows.h>
#define RSF_UNITY_SDK_LEAF rsf_unity_package_vulkan
#include "unity_sdk.h"
#include <IUnityGraphicsVulkan.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif
#include <rescaleframe/foreign_guard.h>

/* Only the Vulkan commands this leaf names cross into the resolver (vulkan_resolver.rs). */
typedef struct rsf_unity_vk_hooks {
    PFN_vkQueuePresentKHR present;
    PFN_vkGetDeviceProcAddr device_proc;
    PFN_vkEnumerateDeviceExtensionProperties enumerate;
    PFN_vkCreateDevice create_device;
    PFN_vkCreateSemaphore create_semaphore;
    PFN_vkDestroySemaphore destroy_semaphore;
    PFN_vkDeviceWaitIdle wait_idle;
    void (*retirement_fault)(void);
} rsf_unity_vk_hooks;
PFN_vkGetInstanceProcAddr rsf_unity_vk_install(PFN_vkGetInstanceProcAddr original, const rsf_unity_vk_hooks* hooks);

/* vulkan_resolver.rs, vulkan_platform.rs and vulkan_bridge.rs read these Vulkan layouts. */
_Static_assert(sizeof(rsf_unity_vk_hooks) == 8 * sizeof(void*), "hook table");
_Static_assert(sizeof(VkExtensionProperties) == 260 && offsetof(VkExtensionProperties, specVersion) == 256,
    "VkExtensionProperties");
_Static_assert(sizeof(VkSwapchainCreateInfoKHR) == 104 && offsetof(VkSwapchainCreateInfoKHR, imageUsage) == 56 &&
    offsetof(VkSwapchainCreateInfoKHR, oldSwapchain) == 96, "VkSwapchainCreateInfoKHR");
_Static_assert(sizeof(VkWin32SurfaceCreateInfoKHR) == 40 && offsetof(VkWin32SurfaceCreateInfoKHR, hwnd) == 32,
    "VkWin32SurfaceCreateInfoKHR");
_Static_assert(sizeof(VkSemaphoreCreateInfo) == 24, "VkSemaphoreCreateInfo");
_Static_assert(sizeof(VkSemaphore) == sizeof(uint64_t), "VkSemaphore");
_Static_assert(sizeof(VkDeviceCreateInfo) == 72 && offsetof(VkDeviceCreateInfo, enabledExtensionCount) == 48 &&
    offsetof(VkDeviceCreateInfo, ppEnabledExtensionNames) == 56, "VkDeviceCreateInfo");
_Static_assert(sizeof(rsf_uvk_engine_instance) == 40 && offsetof(rsf_uvk_engine_instance, queue_family) == 32,
    "engine instance");
_Static_assert(sizeof(rsf_uvk_engine_image) == 16 && offsetof(rsf_uvk_engine_image, layout) == 8, "engine image");
_Static_assert(sizeof(VkPresentInfoKHR) == 64 && offsetof(VkPresentInfoKHR, pSwapchains) == 40, "VkPresentInfoKHR");
_Static_assert(sizeof(rsf_unity_vulkan_device_handles) == 72 && offsetof(rsf_unity_vulkan_device_handles, dispatch) == 64,
    "device handles");

static IUnityGraphicsVulkanV2* unity = NULL;

static void UNITY_INTERFACE_API execute_queue(int event_id, void* event)
{
    (void)event_id;
    rsf_uvk_bridge_execute(event);
}

static VKAPI_ATTR VkResult VKAPI_CALL intercept_present(VkQueue queue, const VkPresentInfoKHR* info)
{
    return (VkResult)rsf_uvk_bridge_present((uint64_t)(uintptr_t)queue, info);
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL instance_lookup(VkInstance instance, const char* name)
{
    RSF_FOREIGN_TRY {
        return vkGetInstanceProcAddr(instance, name);
    } RSF_FOREIGN_CATCH {
        return NULL;
    }
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL device_lookup(VkDevice device, const char* name)
{
    RSF_FOREIGN_TRY {
        return vkGetDeviceProcAddr(device, name);
    } RSF_FOREIGN_CATCH {
        return NULL;
    }
}

static PFN_vkGetInstanceProcAddr UNITY_INTERFACE_API initialize_intercept(PFN_vkGetInstanceProcAddr original, void* user)
{
    (void)user;
    const rsf_unity_vk_hooks hooks = {intercept_present, device_lookup, vkEnumerateDeviceExtensionProperties,
        vkCreateDevice, vkCreateSemaphore, vkDestroySemaphore, vkDeviceWaitIdle, rsf_uvk_bridge_retirement_fault};
    return rsf_unity_vk_install(original, &hooks);
}

bool rsf_uvk_foreign_available(void) { return unity != NULL; }

bool rsf_uvk_foreign_instance(rsf_uvk_engine_instance* output)
{
    if (!unity) return false;
    RSF_FOREIGN_TRY {
        const UnityVulkanInstance instance = unity->Instance();
        output->instance = (uint64_t)(uintptr_t)instance.instance;
        output->device = (uint64_t)(uintptr_t)instance.device;
        output->physical_device = (uint64_t)(uintptr_t)instance.physicalDevice;
        output->queue = (uint64_t)(uintptr_t)instance.graphicsQueue;
        output->queue_family = instance.queueFamilyIndex;
        return true;
    } RSF_FOREIGN_CATCH {
        return false;
    }
}

bool rsf_uvk_foreign_create_device(const rsf_game_host_services* services, rsf_unity_vulkan_device_handles* output)
{
    rsf_uvk_engine_instance instance = {0};
    return rsf_uvk_foreign_instance(&instance) && rsf_uvk_bridge_create_device(services, &instance, output,
        (void (*)(void))instance_lookup, (void (*)(void))device_lookup);
}

bool rsf_uvk_foreign_texture(void* texture, int32_t layout, uint32_t access, rsf_uvk_engine_image* output)
{
    RSF_FOREIGN_TRY {
        UnityVulkanImage image = {0};
        if (!unity->AccessTexture(texture, UnityVulkanWholeImage, (VkImageLayout)layout,
                VK_PIPELINE_STAGE_TRANSFER_BIT, access, kUnityVulkanResourceAccess_PipelineBarrier, &image))
            return false;
        output->image = (uint64_t)(uintptr_t)image.image;
        output->layout = (int32_t)image.layout;
        output->aspect = image.aspect;
        return true;
    } RSF_FOREIGN_CATCH {
        return false;
    }
}

bool rsf_uvk_foreign_queue(int event_id, void* event)
{
    RSF_FOREIGN_TRY {
        unity->AccessQueue(execute_queue, event_id, event, true);
        return true;
    } RSF_FOREIGN_CATCH {
        return false;
    }
}

bool rsf_uvk_foreign_pass(const rsf_game_host_services* services, void* target,
    const rsf_game_render_pass* pass, uint32_t phase)
{
    RSF_FOREIGN_TRY {
        services->render_pass(services->user, target, pass, phase);
        return true;
    } RSF_FOREIGN_CATCH {
        return false;
    }
}

bool rsf_uvk_foreign_present(uint64_t queue, const void* info, int32_t* result)
{
    RSF_FOREIGN_TRY {
        /* The loader export dispatches arbitrary queues, including those outside this bridge. */
        *result = vkQueuePresentKHR((VkQueue)(uintptr_t)queue, (const VkPresentInfoKHR*)info);
        return true;
    } RSF_FOREIGN_CATCH {
        return false;
    }
}

void rsf_uvk_preload(void* interfaces)
{
    RSF_FOREIGN_TRY {
        unity = RSF_UNITY_GET((IUnityInterfaces*)interfaces, IUnityGraphicsVulkanV2);
        if (unity) (void)unity->AddInterceptInitialization(initialize_intercept, NULL, 0);
    } RSF_FOREIGN_CATCH {
        unity = NULL;
    }
}

void rsf_unity_package_sdk_bind_vulkan(void* interfaces, int base_event)
{
    RSF_FOREIGN_TRY {
        unity = RSF_UNITY_GET((IUnityInterfaces*)interfaces, IUnityGraphicsVulkanV2);
        if (!unity) return;
        UnityVulkanPluginEventConfig config = {0};
        config.renderPassPrecondition = kUnityVulkanRenderPass_EnsureOutside;
        config.graphicsQueueAccess = kUnityVulkanGraphicsQueueAccess_DontCare;
        config.flags = kUnityVulkanEventConfigFlag_SyncWorkerThreads;
        for (int offset = 0; offset < 3; ++offset) unity->ConfigureEvent(base_event + offset, &config);
    } RSF_FOREIGN_CATCH {
        unity = NULL;
    }
}

void rsf_unity_package_sdk_unbind_vulkan(void) { unity = NULL; }
