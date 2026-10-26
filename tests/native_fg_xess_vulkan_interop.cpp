// SPDX-License-Identifier: GPL-3.0-only
// Explicit device fixture. Build standalone with the bridge and importer; no
// game, package or cached vendor DLL is modified. Caller owns the GPU lease.
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>
#include "../runtime/orchestrator/src/native_fg_xess_vulkan.h"

extern "C" int VulkanStoryXessFgWaitIdle(rsf_xess_bridge*);
extern "C" int VulkanStoryXessFgDestroyChecked(rsf_xess_bridge*);
static PFN_vkGetInstanceProcAddr instanceProc;
static PFN_vkGetDeviceProcAddr deviceProc;
int RunXessPresentationFixture(const rsf_srvk_setup&, uint32_t, const wchar_t*);
static rsf_vk_function instance_proc(void* object, const char* name) {
    return reinterpret_cast<rsf_vk_function>(instanceProc(static_cast<VkInstance>(object), name));
}
static rsf_vk_function device_proc(void* object, const char* name) {
    return reinterpret_cast<rsf_vk_function>(deviceProc(static_cast<VkDevice>(object), name));
}

int wmain(int argc, wchar_t** argv) {
    // These checks do not load a driver or Intel DLL.
    rsf_xess_bridge* rejected = reinterpret_cast<rsf_xess_bridge*>(1);
    rsf_xess_bridge_create invalid{sizeof(invalid) - 1, RSF_XESS_BRIDGE_ABI_VERSION, 1, nullptr};
    if (RsfXessBridgeCreate(&invalid, &rejected) != RSF_XESS_BRIDGE_INVALID_ARGUMENT || rejected)
        return 10;
    invalid.struct_size = sizeof(invalid);
    invalid.abi_version++;
    if (RsfXessBridgeCreate(&invalid, &rejected) != RSF_XESS_BRIDGE_ABI_MISMATCH || rejected)
        return 11;
    invalid.abi_version = RSF_XESS_BRIDGE_ABI_VERSION;
    invalid.runtime_directory = reinterpret_cast<const uint16_t*>(L"relative");
    if (RsfXessBridgeCreate(&invalid, &rejected) != RSF_XESS_BRIDGE_INVALID_ARGUMENT || rejected)
        return 12;
    if (argc == 1) { std::puts("{\"contract_checks\":\"passed\",\"device_test\":false}"); return 0; }
    if ((argc != 3 && argc != 4) || std::wcscmp(argv[1], L"--device") ||
        (argc == 4 && std::wcscmp(argv[3], L"--present"))) return 13;

    HMODULE loader = LoadLibraryW(L"vulkan-1.dll");
    if (!loader) return 20;
    instanceProc = reinterpret_cast<PFN_vkGetInstanceProcAddr>(GetProcAddress(loader, "vkGetInstanceProcAddr"));
    if (!instanceProc) return 21;
    auto createInstance = reinterpret_cast<PFN_vkCreateInstance>(instanceProc(nullptr, "vkCreateInstance"));
    VkApplicationInfo application{VK_STRUCTURE_TYPE_APPLICATION_INFO};
    application.pApplicationName = "ReScaleFrame XeSS FG interop fixture";
    application.apiVersion = VK_API_VERSION_1_2;
    VkInstanceCreateInfo create{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    create.pApplicationInfo = &application;
    VkInstance instance{};
    VkResult result = createInstance(&create, nullptr, &instance);
    if (result) return 22;
    auto destroyInstance = reinterpret_cast<PFN_vkDestroyInstance>(instanceProc(instance, "vkDestroyInstance"));
    auto enumerate = reinterpret_cast<PFN_vkEnumeratePhysicalDevices>(instanceProc(instance, "vkEnumeratePhysicalDevices"));
    auto getProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(instanceProc(instance, "vkGetPhysicalDeviceProperties2"));
    auto getQueues = reinterpret_cast<PFN_vkGetPhysicalDeviceQueueFamilyProperties>(instanceProc(instance, "vkGetPhysicalDeviceQueueFamilyProperties"));
    auto createDevice = reinterpret_cast<PFN_vkCreateDevice>(instanceProc(instance, "vkCreateDevice"));
    deviceProc = reinterpret_cast<PFN_vkGetDeviceProcAddr>(instanceProc(instance, "vkGetDeviceProcAddr"));
    uint32_t count = 0;
    result = enumerate(instance, &count, nullptr);
    if (result || !count) { destroyInstance(instance, nullptr); return 23; }
    std::vector<VkPhysicalDevice> physicals(count);
    result = enumerate(instance, &count, physicals.data());
    if (result) { destroyInstance(instance, nullptr); return 24; }
    VkPhysicalDevice physical = physicals[0];
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
    properties.pNext = &identity;
    getProperties(physical, &properties);
    uint64_t luid = 0;
    std::memcpy(&luid, identity.deviceLUID, sizeof(luid));
    if (!identity.deviceLUIDValid || !luid) { destroyInstance(instance, nullptr); return 25; }
    count = 0;
    getQueues(physical, &count, nullptr);
    std::vector<VkQueueFamilyProperties> families(count);
    getQueues(physical, &count, families.data());
    uint32_t family = 0;
    while (family < count && !(families[family].queueFlags & VK_QUEUE_GRAPHICS_BIT)) ++family;
    if (family == count) { destroyInstance(instance, nullptr); return 26; }
    float priority = 1;
    VkDeviceQueueCreateInfo queue{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
    queue.queueFamilyIndex = family; queue.queueCount = 1; queue.pQueuePriorities = &priority;
    const char* extensions[] = {VK_KHR_EXTERNAL_MEMORY_WIN32_EXTENSION_NAME,
        VK_KHR_EXTERNAL_SEMAPHORE_WIN32_EXTENSION_NAME};
    VkPhysicalDeviceTimelineSemaphoreFeatures timeline{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES};
    timeline.timelineSemaphore = VK_TRUE;
    VkDeviceCreateInfo deviceInfo{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
    deviceInfo.pNext = &timeline;
    deviceInfo.queueCreateInfoCount = 1; deviceInfo.pQueueCreateInfos = &queue;
    deviceInfo.enabledExtensionCount = 2; deviceInfo.ppEnabledExtensionNames = extensions;
    VkDevice device{};
    result = createDevice(physical, &deviceInfo, nullptr, &device);
    if (result) { destroyInstance(instance, nullptr); return 27; }
    auto destroyDevice = reinterpret_cast<PFN_vkDestroyDevice>(deviceProc(device, "vkDestroyDevice"));
    auto idle = reinterpret_cast<PFN_vkDeviceWaitIdle>(deviceProc(device, "vkDeviceWaitIdle"));
    rsf_xess_bridge_create bridgeInfo{sizeof(bridgeInfo), RSF_XESS_BRIDGE_ABI_VERSION,
        luid, reinterpret_cast<const uint16_t*>(argv[2])};
    rsf_xess_bridge* bridge = nullptr;
    int32_t bridgeResult = RsfXessBridgeCreate(&bridgeInfo, &bridge);
    rsf_xessvk_interop* interop = nullptr;
    int32_t importResult = -1;
    int32_t imageResult[3] = {-1, -1, -1};
    if (!bridgeResult) {
        rsf_srvk_setup setup{};
        setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
        setup.instance = instance; setup.physical_device = physical; setup.device = device;
        setup.instance_proc = instance_proc; setup.device_proc = device_proc;
        importResult = rsf_xessvk_interop_create(&setup, bridge, 1, &interop);
        if (!importResult) {
            const uint32_t formats[] = {VK_FORMAT_B8G8R8A8_UNORM, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT};
            for (uint32_t i = 0; i < 3; ++i) {
                rsf_xessvk_shared_image image{};
                imageResult[i] = rsf_xessvk_interop_image(interop, formats[i], 640, 360, &image);
            }
        }
    }
    int32_t vkDrain = idle(device);
    int32_t dxDrain = bridge ? VulkanStoryXessFgWaitIdle(bridge) : 0;
    int32_t release = 0;
    if (!vkDrain && !dxDrain) release = rsf_xessvk_interop_destroy(interop, 1);
    else release = -1;
    int32_t bridgeRelease = release ? -1 : VulkanStoryXessFgDestroyChecked(bridge);
    std::printf("{\"contract_checks\":\"passed\",\"device_test\":true,\"vendor_id\":%u,\"device_id\":%u,"
        "\"bridge_result\":%d,\"timeline_import\":%d,\"bgra8_import\":%d,\"r32f_import\":%d,\"rg16f_import\":%d,"
        "\"vulkan_drain\":%d,\"dx12_drain\":%d,\"interop_release\":%d,\"bridge_release\":%d}\n",
        properties.properties.vendorID, properties.properties.deviceID, bridgeResult, importResult,
        imageResult[0], imageResult[1], imageResult[2], vkDrain, dxDrain, release, bridgeRelease);
    int presentResult = 0;
    if (!release && !bridgeRelease) {
        if (argc == 4) {
            rsf_srvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
            setup.instance = instance; setup.physical_device = physical; setup.device = device;
            setup.instance_proc = instance_proc; setup.device_proc = device_proc;
            presentResult = RunXessPresentationFixture(setup, family, argv[2]);
        }
        destroyDevice(device, nullptr); destroyInstance(instance, nullptr); FreeLibrary(loader);
    }
    return bridgeResult || importResult || imageResult[0] || imageResult[1] || imageResult[2] || release || bridgeRelease || presentResult ? 1 : 0;
}
