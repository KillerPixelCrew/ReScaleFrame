// SPDX-License-Identifier: GPL-3.0-only
#include "vulkan_creation.h"
#include <rescaleframe/native_xess_vulkan_requirements.h>
#include <atomic>
#include <cstring>
#include <vector>
#include <cstddef>

#if defined(RSF_VULKAN_SR_ENABLED)
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
namespace {
rsf_godot_vk_proc underlying = nullptr;
VkInstance owner_instance = VK_NULL_HANDLE;
std::atomic<bool> prepare_xess{false}, device_ready{false}, external_device_ready{false};
rsf_game_log_fn logger = nullptr; void* logger_user = nullptr;
void diagnostic(const char* value) { if (logger) logger(logger_user, value); }
template<class T> T entry(void* instance, const char* name) { return reinterpret_cast<T>(underlying(instance, name)); }

// Only copy structures used by the pinned engine. An unknown extension keeps
// the original device request intact and makes XeSS unavailable for that device.
size_t feature_size(VkStructureType type) {
    switch (type) {
#define RSF_FEATURE(tag, name) case tag: return sizeof(name)
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2, VkPhysicalDeviceFeatures2);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_1_FEATURES, VkPhysicalDeviceVulkan11Features);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES, VkPhysicalDeviceVulkan12Features);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES, VkPhysicalDeviceVulkan13Features);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES, VkPhysicalDevice16BitStorageFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MULTIVIEW_FEATURES, VkPhysicalDeviceMultiviewFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES, VkPhysicalDeviceShaderFloat16Int8Features);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES, VkPhysicalDeviceBufferDeviceAddressFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_MEMORY_MODEL_FEATURES, VkPhysicalDeviceVulkanMemoryModelFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES, VkPhysicalDeviceTimelineSemaphoreFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PIPELINE_CREATION_CACHE_CONTROL_FEATURES, VkPhysicalDevicePipelineCreationCacheControlFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES, VkPhysicalDeviceSynchronization2Features);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FRAGMENT_DENSITY_MAP_FEATURES_EXT, VkPhysicalDeviceFragmentDensityMapFeaturesEXT);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FRAGMENT_DENSITY_MAP_OFFSET_FEATURES_QCOM, VkPhysicalDeviceFragmentDensityMapOffsetFeaturesQCOM);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FRAGMENT_SHADING_RATE_FEATURES_KHR, VkPhysicalDeviceFragmentShadingRateFeaturesKHR);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FAULT_FEATURES_EXT, VkPhysicalDeviceFaultFeaturesEXT);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ACCELERATION_STRUCTURE_FEATURES_KHR, VkPhysicalDeviceAccelerationStructureFeaturesKHR);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_RAY_TRACING_PIPELINE_FEATURES_KHR, VkPhysicalDeviceRayTracingPipelineFeaturesKHR);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_RAY_TRACING_VALIDATION_FEATURES_NV, VkPhysicalDeviceRayTracingValidationFeaturesNV);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES, VkPhysicalDeviceScalarBlockLayoutFeatures);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES, VkPhysicalDeviceShaderIntegerDotProductFeatures);
#undef RSF_FEATURE
    default: return 0;
    }
}
struct Chain {
    std::vector<std::vector<std::byte>> storage;
    VkBaseOutStructure* head = nullptr;
    bool copy(const void* input) {
        VkBaseOutStructure* tail = nullptr;
        for (auto* node = static_cast<const VkBaseInStructure*>(input); node; node = node->pNext) {
            const auto size = feature_size(node->sType);
            if (!size || storage.size() >= 64 || find(node->sType)) return false;
            storage.emplace_back(size); std::memcpy(storage.back().data(), node, size);
            auto* next = reinterpret_cast<VkBaseOutStructure*>(storage.back().data()); next->pNext = nullptr;
            if (tail) tail->pNext = next; else head = next;
            tail = next;
        }
        return true;
    }
    VkBaseOutStructure* find(VkStructureType type) const {
        for (auto* node = head; node; node = node->pNext) if (node->sType == type) return node;
        return nullptr;
    }
    template<class T> void prepend(T& value) {
        value.pNext = head; head = reinterpret_cast<VkBaseOutStructure*>(&value);
    }
    void feature_root_first() {
        VkBaseOutStructure** link = &head;
        while (*link && (*link)->sType != VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2) link = &(*link)->pNext;
        if (*link && link != &head) {
            auto* root = *link; *link = root->pNext; root->pNext = head; head = root;
        }
    }
};
VkResult VKAPI_PTR create_instance(const VkInstanceCreateInfo* info, const VkAllocationCallbacks* allocator, VkInstance* output) noexcept try {
    const auto create = entry<PFN_vkCreateInstance>(nullptr, "vkCreateInstance");
    if (!create || !info || !output) return VK_ERROR_INITIALIZATION_FAILED;
    auto request = *info;
    VkApplicationInfo app{}; std::vector<const char*> extensions;
    if (prepare_xess) {
        uint32_t count = 0, minimum = 0; const char* const* names = nullptr;
        if (rsf_xessvk_instance_requirements(&count, &names, &minimum) == RSF_BACKEND_OK) {
            if (info->pApplicationInfo) app = *info->pApplicationInfo; else app.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
            if (app.apiVersion < minimum) app.apiVersion = minimum;
            request.pApplicationInfo = &app;
            if (info->enabledExtensionCount) extensions.assign(info->ppEnabledExtensionNames, info->ppEnabledExtensionNames + info->enabledExtensionCount);
            for (uint32_t i = 0; i < count; ++i) {
                bool present = false; for (const char* value : extensions) if (std::strcmp(value, names[i]) == 0) present = true;
                if (!present) extensions.push_back(names[i]);
            }
            request.enabledExtensionCount = static_cast<uint32_t>(extensions.size()); request.ppEnabledExtensionNames = extensions.data();
        } else prepare_xess = false;
    }
    auto result = create(&request, allocator, output);
    if (prepare_xess && (result == VK_ERROR_EXTENSION_NOT_PRESENT || result == VK_ERROR_INCOMPATIBLE_DRIVER)) {
        prepare_xess = false;
        diagnostic("XeSS Vulkan instance requirements unavailable; retrying the original engine instance request.");
        result = create(info, allocator, output);
    }
    if (result == VK_SUCCESS) owner_instance = *output;
    return result;
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }

VkResult create_with_external(PFN_vkCreateDevice create, VkPhysicalDevice physical, const VkDeviceCreateInfo* info,
    const VkAllocationCallbacks* allocator, VkDevice* output) {
    external_device_ready = false;
    const auto enumerate = entry<PFN_vkEnumerateDeviceExtensionProperties>(owner_instance, "vkEnumerateDeviceExtensionProperties");
    uint32_t count = 0;
    if (!enumerate || enumerate(physical, nullptr, &count, nullptr) != VK_SUCCESS || count > 4096) return create(physical, info, allocator, output);
    std::vector<VkExtensionProperties> available(count);
    if (enumerate(physical, nullptr, &count, available.data()) != VK_SUCCESS) return create(physical, info, allocator, output);
    constexpr const char* required[] = {"VK_KHR_external_memory_win32", "VK_KHR_external_semaphore_win32"};
    for (const char* name : required) {
        bool found = false;
        for (const auto& value : available) if (std::strcmp(name, value.extensionName) == 0) found = true;
        if (!found) return create(physical, info, allocator, output);
    }
    bool timeline = false;
    uint32_t visited = 0;
    for (auto* node = static_cast<const VkBaseInStructure*>(info->pNext); node && visited++ < 64; node = node->pNext) {
        if (node->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES)
            timeline |= reinterpret_cast<const VkPhysicalDeviceTimelineSemaphoreFeatures*>(node)->timelineSemaphore != 0;
        if (node->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES)
            timeline |= reinterpret_cast<const VkPhysicalDeviceVulkan12Features*>(node)->timelineSemaphore != 0;
    }
    if (!timeline) return create(physical, info, allocator, output);
    std::vector<const char*> extensions;
    if (info->enabledExtensionCount) extensions.assign(info->ppEnabledExtensionNames, info->ppEnabledExtensionNames + info->enabledExtensionCount);
    for (const char* name : required) {
        bool found = false;
        for (const char* value : extensions) if (std::strcmp(name, value) == 0) found = true;
        if (!found) extensions.push_back(name);
    }
    auto request = *info;
    request.enabledExtensionCount = static_cast<uint32_t>(extensions.size()); request.ppEnabledExtensionNames = extensions.data();
    const auto result = create(physical, &request, allocator, output);
    if (result == VK_ERROR_FEATURE_NOT_PRESENT || result == VK_ERROR_EXTENSION_NOT_PRESENT) {
        diagnostic("Vulkan Win32 interop requirements unavailable; retrying the unchanged device request.");
        return create(physical, info, allocator, output);
    }
    if (result == VK_SUCCESS) {
        external_device_ready = true;
        diagnostic("Vulkan Win32 memory/semaphore interop enabled before device creation.");
    }
    return result;
}
VkResult VKAPI_PTR create_device(VkPhysicalDevice physical, const VkDeviceCreateInfo* info, const VkAllocationCallbacks* allocator, VkDevice* output) noexcept try {
    const auto native_create = entry<PFN_vkCreateDevice>(owner_instance, "vkCreateDevice");
    if (!native_create || !info || !output) return VK_ERROR_INITIALIZATION_FAILED;
    const auto create = [native_create](VkPhysicalDevice target, const VkDeviceCreateInfo* request, const VkAllocationCallbacks* callbacks, VkDevice* result) {
        return create_with_external(native_create, target, request, callbacks, result);
    };
    device_ready = false;
    if (!prepare_xess) return create(physical, info, allocator, output);
    auto fallback = [&]() {
        diagnostic("XeSS Vulkan requirements unavailable; preserving the original engine device request.");
        return create(physical, info, allocator, output);
    };
    Chain chain;
    if (!chain.copy(info->pNext)) return fallback();
    VkPhysicalDeviceFeatures2 base{}; base.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
    if (info->pEnabledFeatures) {
        if (chain.find(base.sType)) return fallback();
        base.features = *info->pEnabledFeatures; chain.prepend(base);
    } else if (!chain.find(base.sType)) chain.prepend(base);
    // The SDK otherwise appends V12/V13 aggregates, which conflict with the
    // engine's existing individual promoted-feature structures.
    VkPhysicalDeviceScalarBlockLayoutFeatures scalar{}; scalar.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES;
    VkPhysicalDeviceShaderIntegerDotProductFeatures dot{}; dot.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES;
    VkPhysicalDeviceShaderFloat16Int8Features int8{}; int8.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES;
    if (!chain.find(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES) && !chain.find(int8.sType)) chain.prepend(int8);
    if (!chain.find(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES) && !chain.find(scalar.sType)) chain.prepend(scalar);
    if (!chain.find(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES) && !chain.find(dot.sType)) chain.prepend(dot);
    chain.feature_root_first();
    void* features = chain.head; uint32_t count = 0; const char* const* names = nullptr;
    if (rsf_xessvk_device_requirements(owner_instance, physical, &count, &names, &features) != RSF_BACKEND_OK) return fallback();
    std::vector<const char*> extensions;
    if (info->enabledExtensionCount) extensions.assign(info->ppEnabledExtensionNames, info->ppEnabledExtensionNames + info->enabledExtensionCount);
    for (uint32_t i = 0; i < count; ++i) {
        bool present = false; for (const char* value : extensions) if (std::strcmp(value, names[i]) == 0) present = true;
        if (!present) extensions.push_back(names[i]);
    }
    auto request = *info; request.pNext = features; request.pEnabledFeatures = nullptr;
    request.enabledExtensionCount = static_cast<uint32_t>(extensions.size()); request.ppEnabledExtensionNames = extensions.data();
    const auto result = create(physical, &request, allocator, output);
    if (result == VK_ERROR_FEATURE_NOT_PRESENT || result == VK_ERROR_EXTENSION_NOT_PRESENT) return fallback();
    if (result == VK_SUCCESS) { device_ready = true; diagnostic("XeSS Vulkan device requirements enabled before device creation."); }
    return result;
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }
rsf_godot_vk_function resolver(void* instance, const char* name) {
    if (!name || !underlying) return nullptr;
    if (std::strcmp(name, "vkCreateInstance") == 0) return reinterpret_cast<rsf_godot_vk_function>(create_instance);
    if (std::strcmp(name, "vkCreateDevice") == 0) {
        if (instance) owner_instance = static_cast<VkInstance>(instance);
        return reinterpret_cast<rsf_godot_vk_function>(create_device);
    }
    return underlying(instance, name);
}
}
#endif
namespace rsf::godot_vk {
rsf_godot_vk_proc creation_resolver(rsf_godot_vk_proc base, bool xess, rsf_game_log_fn log, void* user) {
#if defined(RSF_VULKAN_SR_ENABLED)
    underlying = base; prepare_xess = xess; device_ready = external_device_ready = false; logger = log; logger_user = user;
    return base ? resolver : nullptr;
#else
    (void)xess; (void)log; (void)user; return base;
#endif
}
bool xess_device_ready() {
#if defined(RSF_VULKAN_SR_ENABLED)
    return device_ready;
#else
    return false;
#endif
}
void stop_creation_requirements() {
#if defined(RSF_VULKAN_SR_ENABLED)
    prepare_xess = device_ready = external_device_ready = false;
#endif
}
bool interop_device_ready() {
#if defined(RSF_VULKAN_SR_ENABLED)
    return external_device_ready;
#else
    return false;
#endif
}
}
