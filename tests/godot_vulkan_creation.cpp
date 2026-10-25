// SPDX-License-Identifier: GPL-3.0-only
// Synthetic Vulkan creation boundary. No loader, SDK DLL or GPU is exercised.
#include "../integrations/godot/native/vulkan_creation.cpp"
#include <cstdio>
#include <cstdlib>
#include <string>

namespace {
void require(bool value, const char* message) {
    if (!value) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
const VkInstance fixture_instance = reinterpret_cast<VkInstance>(uintptr_t(11));
const VkPhysicalDevice fixture_physical = reinterpret_cast<VkPhysicalDevice>(uintptr_t(12));
const VkDevice fixture_device = reinterpret_cast<VkDevice>(uintptr_t(13));
const VkDeviceCreateInfo* original_request = nullptr;
const VkAllocationCallbacks* original_allocator = nullptr;
uint32_t device_calls = 0, requirement_calls = 0, instance_calls = 0;
bool sdk_failure = false, instance_failure = false, expect_augmented = true, aggregate_case = false;
bool external_extensions = false;
VkResult first_device_result = VK_SUCCESS;
VkPhysicalDeviceMutableDescriptorTypeFeaturesEXT sdk_mutable{};
void* forwarded_instance = nullptr;
std::string forwarded_name;

const VkBaseInStructure* node(const void* input, VkStructureType type) {
    uint32_t length = 0;
    for (auto* value = static_cast<const VkBaseInStructure*>(input); value; value = value->pNext) {
        require(++length < 64, "feature chain is finite");
        if (value->sType == type) return value;
    }
    return nullptr;
}
template<class T> const T* feature(const void* input, VkStructureType type) {
    return reinterpret_cast<const T*>(node(input, type));
}
uint32_t extension_count(uint32_t count, const char* const* names, const char* name) {
    uint32_t found = 0;
    for (uint32_t i = 0; i < count; ++i) if (std::strcmp(names[i], name) == 0) ++found;
    return found;
}
void VKAPI_PTR unrelated_function() {}
VkResult VKAPI_PTR fake_extensions(VkPhysicalDevice physical, const char*, uint32_t* count, VkExtensionProperties* values) {
    require(physical == fixture_physical, "interop extension query uses the selected physical device");
    if (!external_extensions) { *count = 0; return VK_SUCCESS; }
    if (values) {
        require(*count >= 2, "interop extension query has sufficient storage");
        constexpr char memory_name[] = "VK_KHR_external_memory_win32";
        constexpr char semaphore_name[] = "VK_KHR_external_semaphore_win32";
        std::memcpy(values[0].extensionName, memory_name, sizeof(memory_name));
        std::memcpy(values[1].extensionName, semaphore_name, sizeof(semaphore_name));
    }
    *count = 2; return VK_SUCCESS;
}
VkResult VKAPI_PTR fake_create_instance(const VkInstanceCreateInfo* info, const VkAllocationCallbacks*, VkInstance* output) {
    ++instance_calls;
    require(info && info->pApplicationInfo && info->pApplicationInfo->apiVersion >= VK_API_VERSION_1_3,
        "instance minimum API reaches the selected resolver");
    require(extension_count(info->enabledExtensionCount, info->ppEnabledExtensionNames, "VK_EXT_fixture_instance") == 1,
        "instance requirements deduplicate existing names");
    *output = fixture_instance;
    return VK_SUCCESS;
}
VkResult VKAPI_PTR fake_create_device(VkPhysicalDevice physical, const VkDeviceCreateInfo* info,
    const VkAllocationCallbacks* allocator, VkDevice* output) {
    ++device_calls;
    require(physical == fixture_physical && allocator == original_allocator, "physical device and allocator are forwarded");
    require(info->pQueueCreateInfos == original_request->pQueueCreateInfos && info->queueCreateInfoCount == original_request->queueCreateInfoCount,
        "engine queue request remains borrowed and unchanged");
    if (!expect_augmented || device_calls > 1) {
        require(info == original_request, "fallback forwards the original create-info pointer");
        *output = fixture_device;
        return VK_SUCCESS;
    }
    require(info != original_request && !info->pEnabledFeatures, "prepared device uses a private request and Features2");
    if (external_extensions) {
        require(extension_count(info->enabledExtensionCount, info->ppEnabledExtensionNames, "VK_KHR_external_memory_win32") == 1 &&
            extension_count(info->enabledExtensionCount, info->ppEnabledExtensionNames, "VK_KHR_external_semaphore_win32") == 1,
            "available Win32 interop requirements are merged without duplicates");
    }
    require(extension_count(info->enabledExtensionCount, info->ppEnabledExtensionNames, "VK_KHR_swapchain") == 1 &&
        extension_count(info->enabledExtensionCount, info->ppEnabledExtensionNames, "VK_EXT_mutable_descriptor_type") == 1,
        "required device extensions are merged without duplicates");
    auto* root = feature<VkPhysicalDeviceFeatures2>(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2);
    require(root && info->pNext == root && root->features.shaderClipDistance && root->features.shaderStorageImageWriteWithoutFormat,
        "Features2 is first and preserves caller features beside XeSS requirements");
    auto* mutable_type = feature<VkPhysicalDeviceMutableDescriptorTypeFeaturesEXT>(info->pNext,
        VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MUTABLE_DESCRIPTOR_TYPE_FEATURES_EXT);
    require(mutable_type && mutable_type->mutableDescriptorType, "SDK-owned appended requirement reaches create callback");
    if (aggregate_case) {
        auto* version12 = feature<VkPhysicalDeviceVulkan12Features>(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES);
        auto* version13 = feature<VkPhysicalDeviceVulkan13Features>(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES);
        require(version12 && version12->shaderFloat16 && version12->shaderInt8 && version12->scalarBlockLayout && version12->timelineSemaphore,
            "Vulkan12 preserves engine fields and receives required fields");
        require(version13 && version13->shaderIntegerDotProduct && version13->pipelineCreationCacheControl,
            "Vulkan13 preserves engine fields and receives required fields");
        require(!node(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES) &&
            !node(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES),
            "aggregate features prevent conflicting individual preseeds");
    } else {
        auto* scalar = feature<VkPhysicalDeviceScalarBlockLayoutFeatures>(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES);
        auto* dot = feature<VkPhysicalDeviceShaderIntegerDotProductFeatures>(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES);
        auto* half = feature<VkPhysicalDeviceShaderFloat16Int8Features>(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES);
        require(scalar && scalar->scalarBlockLayout && dot && dot->shaderIntegerDotProduct && half && half->shaderFloat16 && half->shaderInt8,
            "individual features are preseeded and patched while existing fields survive");
        require(!node(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES) &&
            !node(info->pNext, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES),
            "individual feature path has no conflicting aggregate structs");
    }
    if (first_device_result != VK_SUCCESS) return first_device_result;
    *output = fixture_device;
    return VK_SUCCESS;
}
rsf_godot_vk_function fake_gipa(void* instance, const char* name) {
    if (std::strcmp(name, "vkEnumerateDeviceExtensionProperties") == 0)
        return reinterpret_cast<rsf_godot_vk_function>(fake_extensions);
    if (std::strcmp(name, "vkCreateInstance") == 0) {
        require(!instance, "global create-instance resolver uses null instance");
        return reinterpret_cast<rsf_godot_vk_function>(fake_create_instance);
    }
    if (std::strcmp(name, "vkCreateDevice") == 0) {
        require(instance == fixture_instance, "device resolver uses its owner instance");
        return reinterpret_cast<rsf_godot_vk_function>(fake_create_device);
    }
    forwarded_instance = instance; forwarded_name = name;
    return reinterpret_cast<rsf_godot_vk_function>(unrelated_function);
}
PFN_vkCreateDevice begin_device_case(bool enabled = true) {
    device_calls = requirement_calls = 0;
    sdk_failure = false; expect_augmented = enabled; first_device_result = VK_SUCCESS;
    auto resolver_call = rsf::godot_vk::creation_resolver(fake_gipa, enabled, nullptr, nullptr);
    require(resolver_call && !rsf::godot_vk::xess_device_ready(), "new resolver clears readiness");
    return reinterpret_cast<PFN_vkCreateDevice>(resolver_call(fixture_instance, "vkCreateDevice"));
}
}

extern "C" rsf_backend_result rsf_xessvk_instance_requirements(uint32_t* count, const char* const** names, uint32_t* minimum) {
    if (instance_failure) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    static const char* extensions[] = {"VK_EXT_fixture_instance", "VK_EXT_fixture_instance"};
    *count = 2; *names = extensions; *minimum = VK_API_VERSION_1_3;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_xessvk_device_requirements(void* instance, void* physical,
    uint32_t* count, const char* const** names, void** features) {
    ++requirement_calls;
    require(instance == fixture_instance && physical == fixture_physical, "SDK query gets the owner's device identity");
    auto* root = const_cast<VkPhysicalDeviceFeatures2*>(feature<VkPhysicalDeviceFeatures2>(*features, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2));
    require(root && *features == root, "SDK receives writable Features2 at the chain head");
    root->features.shaderStorageImageWriteWithoutFormat = VK_TRUE;
    if (sdk_failure) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (auto* version12 = const_cast<VkPhysicalDeviceVulkan12Features*>(feature<VkPhysicalDeviceVulkan12Features>(*features, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES))) {
        version12->shaderInt8 = version12->scalarBlockLayout = VK_TRUE;
    } else {
        auto* scalar = const_cast<VkPhysicalDeviceScalarBlockLayoutFeatures*>(feature<VkPhysicalDeviceScalarBlockLayoutFeatures>(*features, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES));
        auto* half = const_cast<VkPhysicalDeviceShaderFloat16Int8Features*>(feature<VkPhysicalDeviceShaderFloat16Int8Features>(*features, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES));
        require(scalar && half, "individual scalar feature exists before SDK patching");
        scalar->scalarBlockLayout = half->shaderInt8 = VK_TRUE;
    }
    if (auto* version13 = const_cast<VkPhysicalDeviceVulkan13Features*>(feature<VkPhysicalDeviceVulkan13Features>(*features, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES))) {
        version13->shaderIntegerDotProduct = VK_TRUE;
    } else {
        auto* dot = const_cast<VkPhysicalDeviceShaderIntegerDotProductFeatures*>(feature<VkPhysicalDeviceShaderIntegerDotProductFeatures>(*features, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES));
        require(dot != nullptr, "individual dot-product feature exists before SDK patching"); dot->shaderIntegerDotProduct = VK_TRUE;
    }
    sdk_mutable = {}; sdk_mutable.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MUTABLE_DESCRIPTOR_TYPE_FEATURES_EXT;
    sdk_mutable.mutableDescriptorType = VK_TRUE;
    auto* tail = static_cast<VkBaseOutStructure*>(*features);
    while (tail->pNext) tail = tail->pNext;
    tail->pNext = reinterpret_cast<VkBaseOutStructure*>(&sdk_mutable);
    static const char* extensions[] = {"VK_KHR_swapchain", "VK_EXT_mutable_descriptor_type", "VK_EXT_mutable_descriptor_type"};
    *count = 3; *names = extensions;
    return RSF_BACKEND_OK;
}

int main() {
    require(!rsf::godot_vk::creation_resolver(nullptr, true, nullptr, nullptr), "null underlying resolver remains unavailable");
    auto resolver_call = rsf::godot_vk::creation_resolver(fake_gipa, true, nullptr, nullptr);
    require(!resolver_call(nullptr, nullptr), "null function name is rejected");
    require(resolver_call(fixture_instance, "vkFixtureOther") == reinterpret_cast<rsf_godot_vk_function>(unrelated_function) &&
        forwarded_instance == fixture_instance && forwarded_name == "vkFixtureOther", "unrelated resolver calls are forwarded exactly");
    VkApplicationInfo app{}; app.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO; app.apiVersion = VK_API_VERSION_1_2;
    const char* instance_names[] = {"VK_EXT_fixture_instance"};
    VkInstanceCreateInfo instance_info{}; instance_info.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    instance_info.pApplicationInfo = &app; instance_info.enabledExtensionCount = 1; instance_info.ppEnabledExtensionNames = instance_names;
    auto create_instance_call = reinterpret_cast<PFN_vkCreateInstance>(resolver_call(nullptr, "vkCreateInstance"));
    VkInstance instance = VK_NULL_HANDLE;
    require(create_instance_call(&instance_info, nullptr, &instance) == VK_SUCCESS && instance == fixture_instance && instance_calls == 1,
        "prepared instance forwards to selected resolver");
    require(app.apiVersion == VK_API_VERSION_1_2 && instance_info.pApplicationInfo == &app && instance_info.ppEnabledExtensionNames == instance_names,
        "instance API and extension inputs remain unchanged");

    VkPhysicalDeviceShaderFloat16Int8Features half{}; half.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES; half.shaderFloat16 = VK_TRUE;
    VkPhysicalDeviceTimelineSemaphoreFeatures timeline{}; timeline.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES;
    timeline.timelineSemaphore = VK_TRUE; timeline.pNext = &half;
    VkPhysicalDeviceFeatures enabled{}; enabled.shaderClipDistance = VK_TRUE;
    VkDeviceQueueCreateInfo queue{}; queue.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO; queue.queueFamilyIndex = 4; queue.queueCount = 2;
    const char* device_names[] = {"VK_KHR_swapchain", "VK_EXT_mutable_descriptor_type"};
    VkDeviceCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO; info.pNext = &timeline; info.pEnabledFeatures = &enabled;
    info.queueCreateInfoCount = 1; info.pQueueCreateInfos = &queue; info.enabledExtensionCount = 2; info.ppEnabledExtensionNames = device_names;
    original_request = &info;
    VkAllocationCallbacks allocator{}; original_allocator = &allocator;
    const auto original_info = info; const auto original_half = half; const auto original_timeline = timeline; const auto original_enabled = enabled;
    VkDevice device = VK_NULL_HANDLE;
    auto invoke = [&]() {
        device = VK_NULL_HANDLE;
        return begin_device_case()(fixture_physical, &info, &allocator, &device);
    };
    require(invoke() == VK_SUCCESS && device == fixture_device && rsf::godot_vk::xess_device_ready(), "prepared individual-feature device is ready");
    require(device_calls == 1 && requirement_calls == 1 && !std::memcmp(&info, &original_info, sizeof(info)) &&
        !std::memcmp(&half, &original_half, sizeof(half)) && !std::memcmp(&timeline, &original_timeline, sizeof(timeline)) &&
        !std::memcmp(&enabled, &original_enabled, sizeof(enabled)), "SDK patching never changes caller structures, bits or links");
    rsf::godot_vk::stop_creation_requirements();
    require(!rsf::godot_vk::xess_device_ready(), "creation shutdown clears successful device readiness");

    VkPhysicalDeviceFeatures2 existing_root{}; existing_root.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
    existing_root.features.shaderClipDistance = VK_TRUE; half.pNext = &existing_root; info.pEnabledFeatures = nullptr;
    const auto saved_half = half; const auto saved_root = existing_root;
    require(invoke() == VK_SUCCESS && !std::memcmp(&half, &saved_half, sizeof(half)) &&
        !std::memcmp(&existing_root, &saved_root, sizeof(existing_root)), "existing non-head Features2 is cloned and promoted without caller mutation");

    VkPhysicalDeviceVulkan13Features version13{}; version13.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES;
    version13.pipelineCreationCacheControl = VK_TRUE;
    VkPhysicalDeviceVulkan12Features version12{}; version12.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
    version12.shaderFloat16 = version12.timelineSemaphore = VK_TRUE; version12.pNext = &version13;
    info = original_info; info.pNext = &version12; aggregate_case = true;
    const auto saved12 = version12; const auto saved13 = version13;
    require(invoke() == VK_SUCCESS && !std::memcmp(&version12, &saved12, sizeof(version12)) &&
        !std::memcmp(&version13, &saved13, sizeof(version13)), "aggregate feature path avoids individual duplicates and caller mutation");
    aggregate_case = false; half = original_half; timeline = original_timeline; info = original_info;

    VkBaseInStructure unknown{}; unknown.sType = static_cast<VkStructureType>(0x7ffffffe); info.pNext = &unknown;
    auto call = begin_device_case(); expect_augmented = false;
    require(call(fixture_physical, &info, &allocator, &device) == VK_SUCCESS && device_calls == 1 && !requirement_calls &&
        !rsf::godot_vk::xess_device_ready(), "unknown chain preserves the original request and reports unavailable XeSS");
    info = original_info;

    call = begin_device_case(); sdk_failure = true; expect_augmented = false;
    require(call(fixture_physical, &info, &allocator, &device) == VK_SUCCESS && requirement_calls == 1 &&
        !rsf::godot_vk::xess_device_ready() && !std::memcmp(&enabled, &original_enabled, sizeof(enabled)),
        "SDK refusal after patching a clone falls back without caller mutation");
    for (const auto failure : {VK_ERROR_FEATURE_NOT_PRESENT, VK_ERROR_EXTENSION_NOT_PRESENT}) {
        call = begin_device_case(); first_device_result = failure;
        require(call(fixture_physical, &info, &allocator, &device) == VK_SUCCESS && device_calls == 2 &&
            !rsf::godot_vk::xess_device_ready(), "unsupported augmented request retries original device request");
    }
    call = begin_device_case(); first_device_result = VK_ERROR_OUT_OF_DEVICE_MEMORY;
    require(call(fixture_physical, &info, &allocator, &device) == VK_ERROR_OUT_OF_DEVICE_MEMORY && device_calls == 1 &&
        !rsf::godot_vk::xess_device_ready(), "unrelated create-device failure is preserved without retry");
    call = begin_device_case(false);
    require(call(fixture_physical, &info, &allocator, &device) == VK_SUCCESS && !requirement_calls &&
        !rsf::godot_vk::xess_device_ready(), "disabled XeSS forwards original request directly");
    external_extensions = true;
    call = begin_device_case();
    require(call(fixture_physical, &info, &allocator, &device) == VK_SUCCESS && rsf::godot_vk::interop_device_ready() &&
        !std::memcmp(&timeline, &original_timeline, sizeof(timeline)), "interop is prepared before device creation without changing the engine's feature chain");
    external_extensions = false;
    rsf::godot_vk::stop_creation_requirements();
    require(!rsf::godot_vk::xess_device_ready() && !rsf::godot_vk::interop_device_ready(), "creation shutdown clears readiness");
    std::puts("PASS: Godot Vulkan creation clone, requirements, fallback and resolver boundary");
    return 0;
}
