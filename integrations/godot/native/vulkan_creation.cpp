// SPDX-License-Identifier: MIT
#include "vulkan_creation.h"
#include "godot_platform.h"
#include <rescaleframe/native_xess_vulkan_requirements.h>
#include <rescaleframe/native_latency_vulkan_requirements.h>
#include <cstring>
#include <vector>
#include <cstddef>

// The resolver's routes, the stage order (XeSS, Win32 interop, core latency, native create), the
// extension lists, the feature structures XeSS needs added, every retry and the XeSS and interop
// readiness are owned by the creation owner in native/vulkan/creation.rs. This file keeps the
// Vulkan entry points the resolver hands out, which forward to it, and the typed leaves it calls:
// the feature chain facts and clones, request copies, the XeSS and core latency requirement calls
// and the Vulkan create and destroy calls (godot_platform.h, rsf_godot_vk_creation_leaves).
static_assert(sizeof(rsf_godot_vk_chain_facts) == 40 && sizeof(rsf_godot_vk_device_derive) == 40 &&
    offsetof(rsf_godot_vk_device_derive, next) == 24 && sizeof(rsf_godot_vk_latency_requirements) == 24 &&
    sizeof(rsf_godot_vk_instance_call) == 48 && sizeof(rsf_godot_vk_device_call) == 56 &&
    offsetof(rsf_godot_vk_device_call, extension_count) == 48 && sizeof(rsf_godot_vk_creation_leaves) == 112 &&
    offsetof(rsf_godot_vk_creation_leaves, latency_requirements) == 64 &&
    offsetof(rsf_godot_vk_creation_leaves, destroy_device) == 104,
    "native/platform.rs mirrors these layouts");

#if defined(RSF_VULKAN_SR_ENABLED)
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
static_assert(VK_SUCCESS == 0 && VK_ERROR_OUT_OF_HOST_MEMORY == -1 && VK_ERROR_INITIALIZATION_FAILED == -3 &&
    VK_ERROR_EXTENSION_NOT_PRESENT == -7 && VK_ERROR_FEATURE_NOT_PRESENT == -8 && VK_ERROR_INCOMPATIBLE_DRIVER == -9 &&
    RSF_BACKEND_OK == 0, "native/vulkan/creation.rs mirrors these results");
namespace {
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
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD, VkPhysicalDeviceAntiLagFeaturesAMD);
    RSF_FEATURE(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MUTABLE_DESCRIPTOR_TYPE_FEATURES_EXT, VkPhysicalDeviceMutableDescriptorTypeFeaturesEXT);
#undef RSF_FEATURE
    default: return 0;
    }
}
constexpr size_t chain_limit = 64;
struct Chain {
    std::vector<std::vector<std::byte>> storage;
    VkBaseOutStructure* head = nullptr;
    bool copy(const void* input) {
        VkBaseOutStructure* tail = nullptr;
        for (auto* node = static_cast<const VkBaseInStructure*>(input); node; node = node->pNext) {
            const auto size = feature_size(node->sType);
            if (!size || storage.size() >= chain_limit || find(node->sType)) return false;
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

// One vkCreateInstance or vkCreateDevice. Everything a derived request points into lives here
// until the entry point returns; the extension lists are the owner's, borrowed for the call.
struct InstanceRecord {
    VkApplicationInfo app{};
    VkInstanceCreateInfo request{};
};
struct DeviceRecord {
    Chain xess, latency;
    VkPhysicalDeviceFeatures2 base{};
    VkPhysicalDeviceScalarBlockLayoutFeatures scalar{};
    VkPhysicalDeviceShaderIntegerDotProductFeatures dot{};
    VkPhysicalDeviceShaderFloat16Int8Features int8{};
    VkDeviceCreateInfo stages[3]{};
};
InstanceRecord& instance_of(void* call) { return *static_cast<InstanceRecord*>(call); }
DeviceRecord& device_of(void* call) { return *static_cast<DeviceRecord*>(call); }
const VkDeviceCreateInfo& device_request(const void* request) { return *static_cast<const VkDeviceCreateInfo*>(request); }

int32_t leaf_xess_instance_requirements(uint32_t* count, const char* const** names, uint32_t* minimum) noexcept try {
    return rsf_xessvk_instance_requirements(count, names, minimum);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
const void* leaf_derive_instance(void* call, const void* base, uint32_t minimum, const char* const* extensions,
    uint32_t count) noexcept try {
    auto& value = instance_of(call);
    const auto& info = *static_cast<const VkInstanceCreateInfo*>(base);
    value.app = {};
    if (info.pApplicationInfo) value.app = *info.pApplicationInfo; else value.app.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    if (value.app.apiVersion < minimum) value.app.apiVersion = minimum;
    value.request = info; value.request.pApplicationInfo = &value.app;
    value.request.enabledExtensionCount = count; value.request.ppEnabledExtensionNames = extensions;
    return &value.request;
} catch (...) { return nullptr; }
int32_t leaf_create_instance(rsf_godot_vk_function create, const void* request, const void* allocator, void* output) noexcept try {
    return reinterpret_cast<PFN_vkCreateInstance>(create)(static_cast<const VkInstanceCreateInfo*>(request),
        static_cast<const VkAllocationCallbacks*>(allocator), static_cast<VkInstance*>(output));
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }
void leaf_chain_facts(const void* request, rsf_godot_vk_chain_facts* facts) noexcept try {
    *facts = {};
    const auto& info = device_request(request);
    facts->enabled_features = info.pEnabledFeatures ? 1u : 0u;
    // The clone's admission (Chain::copy) without copying.
    VkStructureType seen[chain_limit]{};
    size_t count = 0;
    bool cloneable = true;
    for (auto* node = static_cast<const VkBaseInStructure*>(info.pNext); node; node = node->pNext) {
        if (!feature_size(node->sType) || count >= chain_limit) { cloneable = false; break; }
        for (size_t i = 0; i < count; ++i) if (seen[i] == node->sType) cloneable = false;
        if (!cloneable) break;
        seen[count++] = node->sType;
    }
    if (cloneable) {
        facts->cloneable = 1;
        for (size_t i = 0; i < count; ++i) switch (seen[i]) {
        case VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2: facts->features2 = 1; break;
        case VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES: facts->vulkan12 = 1; break;
        case VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES: facts->vulkan13 = 1; break;
        case VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES: facts->float16_int8 = 1; break;
        case VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES: facts->scalar_block_layout = 1; break;
        case VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES: facts->integer_dot_product = 1; break;
        default: break;
        }
    }
    uint32_t visited = 0;
    for (auto* node = static_cast<const VkBaseInStructure*>(info.pNext); node && visited++ < chain_limit; node = node->pNext) {
        if (node->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES &&
            reinterpret_cast<const VkPhysicalDeviceTimelineSemaphoreFeatures*>(node)->timelineSemaphore)
            facts->timeline_semaphore = 1;
        if (node->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES &&
            reinterpret_cast<const VkPhysicalDeviceVulkan12Features*>(node)->timelineSemaphore)
            facts->timeline_semaphore = 1;
    }
} catch (...) { *facts = {}; }
uint32_t leaf_xess_features(void* call, const void* request, uint32_t structures, void** features) noexcept try {
    auto& value = device_of(call);
    const auto& info = device_request(request);
    *features = nullptr;
    value.xess = Chain{};
    if (!value.xess.copy(info.pNext)) return 1;
    value.base = {}; value.base.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
    if ((structures & RSF_GODOT_VK_XESS_ENABLED_FEATURES) && info.pEnabledFeatures) {
        value.base.features = *info.pEnabledFeatures; value.xess.prepend(value.base);
    } else if (structures & RSF_GODOT_VK_XESS_FEATURES2) value.xess.prepend(value.base);
    // The SDK otherwise appends V12/V13 aggregates, which conflict with the
    // engine's existing individual promoted-feature structures.
    value.int8 = {}; value.int8.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES;
    value.scalar = {}; value.scalar.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES;
    value.dot = {}; value.dot.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES;
    if (structures & RSF_GODOT_VK_XESS_FLOAT16_INT8) value.xess.prepend(value.int8);
    if (structures & RSF_GODOT_VK_XESS_SCALAR_BLOCK_LAYOUT) value.xess.prepend(value.scalar);
    if (structures & RSF_GODOT_VK_XESS_INTEGER_DOT_PRODUCT) value.xess.prepend(value.dot);
    value.xess.feature_root_first();
    *features = value.xess.head;
    return 1;
} catch (...) { return 0; }
int32_t leaf_xess_device_requirements(void* instance, void* physical, uint32_t* count, const char* const** names,
    void** features) noexcept try {
    return rsf_xessvk_device_requirements(instance, physical, count, names, features);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
uint32_t leaf_device_extensions(rsf_godot_vk_function function, void* physical, const char* const* names, uint32_t count) noexcept try {
    const auto enumerate = reinterpret_cast<PFN_vkEnumerateDeviceExtensionProperties>(function);
    const auto device = static_cast<VkPhysicalDevice>(physical);
    uint32_t available_count = 0;
    if (enumerate(device, nullptr, &available_count, nullptr) != VK_SUCCESS || available_count > 4096) return 0;
    std::vector<VkExtensionProperties> available(available_count);
    if (enumerate(device, nullptr, &available_count, available.data()) != VK_SUCCESS) return 0;
    for (uint32_t i = 0; i < count; ++i) {
        bool found = false;
        for (const auto& value : available) if (std::strcmp(names[i], value.extensionName) == 0) found = true;
        if (!found) return 0;
    }
    return 1;
} catch (...) { return 0; }
uint32_t leaf_latency_requirements(void* call, void* instance, void* physical, rsf_godot_vk_proc proc, const void* request,
    rsf_godot_vk_latency_requirements* requirements) noexcept try {
    auto& value = device_of(call);
    *requirements = {RSF_BACKEND_ERROR_NOT_SUPPORTED, 0, nullptr, nullptr};
    value.latency = Chain{};
    if (!value.latency.copy(device_request(request).pNext)) return 1;
    const rsf_latencyvk_device_query query{sizeof(query), 1, instance, physical, proc, value.latency.head};
    rsf_latencyvk_device_requirements answer{sizeof(answer), 1};
    requirements->result = rsf_latencyvk_require_device(&query, &answer);
    requirements->extension_count = answer.extension_count;
    requirements->extensions = answer.extensions;
    requirements->features = answer.features;
    return 1;
} catch (...) { return 0; }
const void* leaf_derive_device(void* call, uint32_t stage, const rsf_godot_vk_device_derive* derive) noexcept try {
    if (stage > RSF_GODOT_VK_STAGE_LATENCY) return nullptr;
    auto& request = device_of(call).stages[stage];
    request = device_request(derive->base);
    if (derive->replace_next) request.pNext = derive->next;
    if (derive->clear_enabled_features) request.pEnabledFeatures = nullptr;
    request.enabledExtensionCount = derive->extension_count; request.ppEnabledExtensionNames = derive->extensions;
    return &request;
} catch (...) { return nullptr; }
int32_t leaf_create_device(rsf_godot_vk_function create, void* physical, const void* request, const void* allocator,
    void* output) noexcept try {
    return reinterpret_cast<PFN_vkCreateDevice>(create)(static_cast<VkPhysicalDevice>(physical), &device_request(request),
        static_cast<const VkAllocationCallbacks*>(allocator), static_cast<VkDevice*>(output));
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }
int32_t leaf_latency_created(void* instance, void* physical, void* device, rsf_godot_vk_proc proc, const void* request) noexcept try {
    const rsf_latencyvk_created_device facts{sizeof(facts), 1, instance, physical, device, proc, request};
    return rsf_latencyvk_device_created(&facts);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
void leaf_latency_destroyed(void* device) noexcept try {
    rsf_latencyvk_device_destroyed(device);
} catch (...) {}
void leaf_destroy_device(rsf_godot_vk_function destroy, void* device, const void* allocator) noexcept try {
    reinterpret_cast<PFN_vkDestroyDevice>(destroy)(static_cast<VkDevice>(device), static_cast<const VkAllocationCallbacks*>(allocator));
} catch (...) {}

const rsf_godot_vk_creation_leaves creation_leaves{sizeof(creation_leaves), RSF_GODOT_VK_CREATION_VERSION,
    leaf_xess_instance_requirements, leaf_derive_instance, leaf_create_instance, leaf_chain_facts, leaf_xess_features,
    leaf_xess_device_requirements, leaf_device_extensions, leaf_latency_requirements, leaf_derive_device,
    leaf_create_device, leaf_latency_created, leaf_latency_destroyed, leaf_destroy_device};

// The entry points the resolver hands out. Each forwards to the creation owner with its record.
VkResult VKAPI_PTR create_instance(const VkInstanceCreateInfo* info, const VkAllocationCallbacks* allocator, VkInstance* output) noexcept try {
    InstanceRecord record;
    const rsf_godot_vk_instance_call call{&record, info, allocator, output,
        info ? info->ppEnabledExtensionNames : nullptr, info ? info->enabledExtensionCount : 0u, 0};
    return static_cast<VkResult>(rsf_godot_vk_create_instance(&creation_leaves, &call));
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }
VkResult VKAPI_PTR create_device(VkPhysicalDevice physical, const VkDeviceCreateInfo* info, const VkAllocationCallbacks* allocator, VkDevice* output) noexcept try {
    DeviceRecord record;
    const rsf_godot_vk_device_call call{&record, physical, info, allocator, output,
        info ? info->ppEnabledExtensionNames : nullptr, info ? info->enabledExtensionCount : 0u, 0};
    return static_cast<VkResult>(rsf_godot_vk_create_device(&creation_leaves, &call));
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }
void VKAPI_PTR destroy_device(VkDevice device, const VkAllocationCallbacks* allocator) {
    rsf_godot_vk_destroy_device(&creation_leaves, device, allocator);
}
rsf_godot_vk_function route(uint32_t value, rsf_godot_vk_function forwarded);
PFN_vkVoidFunction VKAPI_PTR device_proc(VkDevice device, const char* name) {
    rsf_godot_vk_function forwarded = nullptr;
    const auto value = rsf_godot_vk_creation_resolve(RSF_GODOT_VK_SCOPE_DEVICE, device, name, &forwarded);
    return reinterpret_cast<PFN_vkVoidFunction>(route(value, forwarded));
}
// The entry point of an owner route.
rsf_godot_vk_function route(uint32_t value, rsf_godot_vk_function forwarded) {
    switch (value) {
    case RSF_GODOT_VK_ROUTE_FORWARD: return forwarded;
    case RSF_GODOT_VK_ROUTE_CREATE_INSTANCE: return reinterpret_cast<rsf_godot_vk_function>(create_instance);
    case RSF_GODOT_VK_ROUTE_DEVICE_PROC: return reinterpret_cast<rsf_godot_vk_function>(device_proc);
    case RSF_GODOT_VK_ROUTE_CREATE_DEVICE: return reinterpret_cast<rsf_godot_vk_function>(create_device);
    case RSF_GODOT_VK_ROUTE_DESTROY_DEVICE: return reinterpret_cast<rsf_godot_vk_function>(destroy_device);
    default: return nullptr;
    }
}
rsf_godot_vk_function resolver(void* instance, const char* name) {
    rsf_godot_vk_function forwarded = nullptr;
    const auto value = rsf_godot_vk_creation_resolve(RSF_GODOT_VK_SCOPE_INSTANCE, instance, name, &forwarded);
    return route(value, forwarded);
}
}
#endif
namespace rsf::godot_vk {
rsf_godot_vk_proc creation_resolver(rsf_godot_vk_proc base, bool xess, rsf_game_log_fn log, void* user) {
#if defined(RSF_VULKAN_SR_ENABLED)
    return rsf_godot_vk_creation_begin(base, xess ? 1u : 0u, log, user) ? resolver : nullptr;
#else
    (void)xess; (void)log; (void)user; return base;
#endif
}
bool xess_device_ready() {
#if defined(RSF_VULKAN_SR_ENABLED)
    return rsf_godot_vk_xess_ready() != 0;
#else
    return false;
#endif
}
void stop_creation_requirements() {
#if defined(RSF_VULKAN_SR_ENABLED)
    rsf_godot_vk_creation_stop();
#endif
}
bool interop_device_ready() {
#if defined(RSF_VULKAN_SR_ENABLED)
    return rsf_godot_vk_interop_ready() != 0;
#else
    return false;
#endif
}
}
