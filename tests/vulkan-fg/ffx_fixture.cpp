// SPDX-License-Identifier: MIT
// The production WSI hooks borrow the native leaf from the Rust route. A hook
// inside its forwarded Vulkan call keeps the leaf. Rust unregisters
// before closing the native leaf and releasing process-wide admission.
#include "../../runtime/orchestrator/src/native_fg_vulkan.cpp"
#include "ffx_abi.h"
#include <cstdio>
#include <cstdlib>

extern "C" int32_t rsf_slvk_without_dlssg(int32_t (*call)(void*), void* user) { return call(user); }
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call call, void* user) { return call(user); }
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function call, void* queue, const void* info) {
    return reinterpret_cast<PFN_vkQueuePresentKHR>(call)(static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
}

extern "C" {
VkResult VKAPI_PTR rsf_fixture_ffx_images(VkDevice, VkSwapchainKHR, uint32_t*, VkImage*);
VkResult VKAPI_PTR rsf_fixture_ffx_acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
uint64_t rsf_fixture_ffx_present_count(VkSwapchainKHR);
}

namespace fake {
inline void require(bool value, const char* text) { if (!value) { std::fprintf(stderr, "%s\n", text); std::exit(1); } }
template<class T> T id(uint64_t value) { return reinterpret_cast<T>(static_cast<uintptr_t>(value)); }

struct NativeFixture {
    FfxApiFrameGenerationDispatchFunc callback = nullptr;
    void* context = nullptr;
    rsf_ffxgvk_native* native = nullptr;
} native_fixture{};

ffxReturnCode_t dispatch(ffxContext* context, const ffxDispatchDescHeader* header) {
    switch (header->type) {
    case FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE:
        require(context == &native_fixture.native->effect, "prepare reaches the effect context");
        ++rsf_fixture_ffx_state()->sdk_records; // A failing SDK call can already have recorded GPU work.
        return rsf_fixture_ffx_state()->reject_prepare ? FFX_API_RETURN_ERROR_PARAMETER : FFX_API_RETURN_OK;
    case FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION: {
        const auto& request = *reinterpret_cast<const ffxDispatchDescFrameGeneration*>(header);
        if (rsf_fixture_ffx_state()->check_reset) require(request.reset == rsf_fixture_ffx_state()->expected_reset, "camera reset reaches SDK dispatch without changing activation");
        if (rsf_fixture_ffx_state()->reject_generate) return FFX_API_RETURN_ERROR;
        rsf_fixture_ffx_state()->generated_frame = request.frameID;
        ++rsf_fixture_ffx_state()->generated; return FFX_API_RETURN_OK;
    }
    case FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK:
        require(context == &native_fixture.native->swapchain && rsf_fixture_ffx_state()->locks == rsf_fixture_ffx_state()->unlocks + 1, "presents drain on the swapchain under the queue lock");
        ++rsf_fixture_ffx_state()->drains; return rsf_fixture_ffx_state()->reject_drain ? FFX_API_RETURN_ERROR : FFX_API_RETURN_OK;
    default:
        require(false, "unexpected SDK dispatch"); return FFX_API_RETURN_ERROR;
    }
}
ffxReturnCode_t configure(ffxContext* context, const ffxConfigureDescHeader* header) {
    const auto& request = *reinterpret_cast<const ffxConfigureDescFrameGeneration*>(header);
    require(context == &native_fixture.native->effect && request.header.type == FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION &&
        request.swapChain == native_fixture.native->chain && !request.allowAsyncWorkloads, "configure targets the owned presenter");
    const bool enabled = request.frameGenerationEnabled;
    if (enabled) require(request.frameGenerationCallback && request.frameGenerationCallbackUserContext == native_fixture.native &&
        request.HUDLessColor.resource, "enabled presenter receives its callback and leased HUD-less input");
    if (rsf_fixture_ffx_state()->enabled != enabled) ++rsf_fixture_ffx_state()->activations;
    rsf_fixture_ffx_state()->enabled = enabled; rsf_fixture_ffx_state()->configured_frame = request.frameID;
    native_fixture.callback = request.frameGenerationCallback; native_fixture.context = request.frameGenerationCallbackUserContext;
    ++rsf_fixture_ffx_state()->configurations; return FFX_API_RETURN_OK;
}
ffxReturnCode_t destroy_context(ffxContext* context, const ffxAllocationCallbacks*) {
    if (context == &native_fixture.native->effect) {
        if (rsf_fixture_ffx_state()->reject_effect_destroy) return FFX_API_RETURN_ERROR;
        ++rsf_fixture_ffx_state()->effects_destroyed;
    } else {
        require(context == &native_fixture.native->swapchain && !native_fixture.native->chain, "swapchain context outlives its chain");
        if (rsf_fixture_ffx_state()->reject_swapchain_destroy) return FFX_API_RETURN_ERROR;
        ++rsf_fixture_ffx_state()->swapchains_destroyed;
    }
    return FFX_API_RETURN_OK;
}
VkResult VKAPI_PTR present(VkQueue queue, const VkPresentInfoKHR*) {
    require(queue == static_cast<VkQueue>(native_fixture.native->setup.queues[0]), "presents through the graphics queue");
    ++rsf_fixture_ffx_state()->present_calls;
    if (rsf_fixture_ffx_state()->enabled) {
        // FrameInterpolationSwapChainVK calls the generation callback inside queuePresent.
        ffxDispatchDescFrameGeneration request{}; request.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION;
        request.frameID = rsf_fixture_ffx_state()->configured_frame + 1;
        require(native_fixture.callback(&request, native_fixture.context) == FFX_API_RETURN_ERROR_PARAMETER,
            "generation callback refuses a different frame than the configured source");
        request.frameID = rsf_fixture_ffx_state()->configured_frame; request.numGeneratedFrames = 1;
        const ffxReturnCode_t expected = rsf_fixture_ffx_state()->reject_generate ? FFX_API_RETURN_ERROR : FFX_API_RETURN_OK;
        require(native_fixture.callback(&request, native_fixture.context) == expected,
            "generation callback returns the SDK result for the leased source frame");
    }
    return VkResult(rsf_fixture_ffx_state()->present_result);
}
void VKAPI_PTR destroy_chain(VkDevice, VkSwapchainKHR chain, const VkAllocationCallbacks*, void* context) {
    require(chain && context == native_fixture.native->swapchain, "chain destroyed with its swapchain context"); ++rsf_fixture_ffx_state()->chains_destroyed;
}

// The leaf the Rust WSI route lends to a hook on the fixture device, or null.
inline rsf_ffxgvk_native* routed() {
    auto* native = rsf_core_ffxgvk_wsi_borrow(id<void*>(1), 1);
    if (native) rsf_core_ffxgvk_wsi_return();
    return native;
}

// Fake open replaces module loading and Vulkan entry resolution. Admission
// and the order of WSI registration and teardown belong to the real Rust route.
int32_t open(const rsf_fgvk_setup* setup, void*, uint32_t width, uint32_t height, rsf_ffxgvk_native** out, void** resources) {
    require(!routed(), "Rust publishes the leaf only after open");
    auto* self = new rsf_ffxgvk_native{};
    self->setup = *setup; self->width = width; self->height = height;
    self->sdk.destroy = destroy_context; self->sdk.dispatch = dispatch; self->configure = configure;
    const auto device = setup->graphics.device;
    self->barrier = reinterpret_cast<PFN_vkCmdPipelineBarrier>(rsf_fixture_ffx_lookup(device, "vkCmdPipelineBarrier"));
    self->copy = reinterpret_cast<PFN_vkCmdCopyImage>(rsf_fixture_ffx_lookup(device, "vkCmdCopyImage"));
    self->idle = reinterpret_cast<PFN_vkDeviceWaitIdle>(rsf_fixture_ffx_lookup(device, "vkDeviceWaitIdle"));
    self->setup.graphics.device_proc = rsf_fixture_ffx_lookup;
    self->gpu.reset(rsf_core_vkfg_load_device(&self->setup.graphics, &self->setup, false));
    require(self->gpu != nullptr, "fake Vulkan device entries loaded");
    VkPhysicalDeviceMemoryProperties memory{};
    memory.memoryTypeCount = 1;
    memory.memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    rsf_core_vkfg_device_memory(self->gpu.get(), &memory);
    native_fixture.native = self; *resources = self->gpu.get(); *out = self;
    return RSF_BACKEND_OK;
}
int32_t create_swapchain(rsf_ffxgvk_native* self, uint64_t* chain, uint32_t* has_context) {
    self->swapchain = id<ffxContext>(4); self->chain = id<VkSwapchainKHR>(3); *chain = 3;
    *has_context = 1;
    return RSF_BACKEND_OK;
}
int32_t query_swapchain(rsf_ffxgvk_native* self) {
    auto& functions = self->functions;
    functions.pOutQueuePresentKHR = present; functions.pOutGetSwapchainImagesKHR = rsf_fixture_ffx_images;
    functions.pOutAcquireNextImageKHR = rsf_fixture_ffx_acquire; functions.pOutDestroySwapchainFFXAPI = destroy_chain;
    functions.pOutGetLastPresentCountFFXAPI = rsf_fixture_ffx_present_count;
    return RSF_BACKEND_OK;
}
int32_t create_effect(rsf_ffxgvk_native* self, uint32_t* has_context) {
    self->effect = id<ffxContext>(5); *has_context = 1; return RSF_BACKEND_OK;
}
int32_t query_provider(rsf_ffxgvk_native*) { return RSF_BACKEND_OK; }

inline rsf_ffxgvk_ops ops() {
    auto value = operations;
    value.loader = {[](const uint16_t*) -> void* { return id<void*>(9); }, [](void*) {}};
    value.open = open; value.create_swapchain = create_swapchain; value.query_swapchain = query_swapchain;
    value.create_effect = create_effect; value.query_provider = query_provider;
    value.set_wsi = [](rsf_ffxgvk_native*, uint32_t) -> int32_t { return RSF_BACKEND_OK; };
    return value;
}

struct Setup {
    VkSwapchainCreateInfoKHR info{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
    rsf_fgvk_setup value{sizeof(value), RSF_FGVK_ABI_VERSION};
    Setup() {
        info.imageExtent = {640, 360};
        info.imageFormat = VK_FORMAT_R8G8B8A8_UNORM;
        info.imageColorSpace = VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
        info.imageArrayLayers = 1;
        auto& graphics = value.graphics;
        graphics.struct_size = sizeof(graphics); graphics.abi_version = RSF_SRVK_ABI_VERSION;
        graphics.instance = id<void*>(7); graphics.physical_device = id<void*>(8); graphics.device = id<void*>(1);
        graphics.instance_proc = [](void*, const char*) -> rsf_vk_function { return nullptr; };
        graphics.device_proc = graphics.instance_proc; graphics.runtime_directory_utf8 = "C:\\fake";
        graphics.depth_inverted = 1;
        for (uint32_t i = 0; i < 4; ++i) value.queues[i] = id<void*>(20 + i);
        value.create_info = &info; value.graphics_submit = rsf_fixture_ffx_submit; value.lock_queues = rsf_fixture_ffx_lock; value.unlock_queues = rsf_fixture_ffx_unlock;
    }
};

}

using namespace fake;

namespace route {
int32_t wsi(const rsf_ffx_wsi_dispatch* setup) {
    require(setup && setup->struct_size == sizeof(*setup) && setup->abi_version == 2 && setup->device == id<VkDevice>(1),
        "the dispatch uses this device and the corrected SDK ABI");
    auto* fixture = rsf_fixture_wsi_route_state();
    if (setup->create) {
        require(setup->create == serialized_create && setup->destroy == serialized_destroy &&
            setup->present == serialized_present && setup->acquire == serialized_acquire && setup->images == serialized_images,
            "registration publishes the typed host hooks");
        require(!routed(), "the route publishes its leaf only after registration succeeds");
        ++fixture->registered;
        return 0;
    }
    require(!setup->destroy && !setup->present && !setup->acquire && !setup->images, "teardown clears every hook");
    require(!routed(), "the route lends no leaf during unregistration");
    require(rsf_fixture_ffx_refused_second_owner() == RSF_BACKEND_ERROR_NOT_SUPPORTED, "admission is held through unregistration");
    if (fixture->refuse_unregister) return -1;
    ++fixture->unregistered; return 0;
}

int32_t open(const rsf_fgvk_setup* setup, void* module, uint32_t width, uint32_t height, rsf_ffxgvk_native** out, void** resources) {
    const auto result = fake::open(setup, module, width, height, out, resources);
    if (result != RSF_BACKEND_OK) return result;
    auto* native = *out;
    native->native_present = reinterpret_cast<PFN_vkQueuePresentKHR>(rsf_fixture_wsi_entry("vkQueuePresentKHR"));
    native->native_create = reinterpret_cast<PFN_vkCreateSwapchainKHR>(rsf_fixture_wsi_entry("vkCreateSwapchainKHR"));
    native->native_destroy = reinterpret_cast<PFN_vkDestroySwapchainKHR>(rsf_fixture_wsi_entry("vkDestroySwapchainKHR"));
    native->native_acquire = reinterpret_cast<PFN_vkAcquireNextImageKHR>(rsf_fixture_wsi_entry("vkAcquireNextImageKHR"));
    native->native_images = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(rsf_fixture_wsi_entry("vkGetSwapchainImagesKHR"));
    native->setup.graphics.device_proc = rsf_fixture_wsi_lookup;
    native->wsi_dispatch = wsi;
    return RSF_BACKEND_OK;
}
}


extern "C" int32_t rsf_fixture_ffx_start(bool routed_fixture, rsf_ffxgvk** out, uint64_t* chain) {
    Setup setup;
    static const auto ordinary = ops();
    static const auto routed_ops = [] {
        auto table = ops();
        table.open = route::open;
        table.set_wsi = set_wsi;
        return table;
    }();
    const auto result = rsf_core_ffxgvk_create_native(routed_fixture ? &routed_ops : &ordinary, &setup.value, out, chain);
    if (routed_fixture && result == RSF_BACKEND_OK) rsf_fixture_wsi_route_state()->owner = *out;
    return result;
}
extern "C" void* rsf_fixture_ffx_native() { return native_fixture.native; }
extern "C" void* rsf_fixture_ffx_routed() { return routed(); }
extern "C" uint64_t rsf_fixture_ffx_chain() { return handle_value(native_fixture.native->chain); }
extern "C" void* rsf_fixture_ffx_gpu() { return native_fixture.native->gpu.get(); }
extern "C" const rsf_vkfg_platform* rsf_fixture_ffx_platform() { return &rsf_core_vkfg_platform; }
extern "C" int32_t rsf_fixture_wsi_create(uint64_t device, uint64_t* out) {
    VkSwapchainKHR chain{};
    const auto result = serialized_create(id<VkDevice>(device), nullptr, nullptr, &chain);
    if (result == VK_SUCCESS) *out = handle_value(chain);
    return result;
}
extern "C" void rsf_fixture_wsi_destroy(uint64_t device, uint64_t chain) {
    serialized_destroy(id<VkDevice>(device), id<VkSwapchainKHR>(chain), nullptr);
}
extern "C" int32_t rsf_fixture_wsi_acquire(uint64_t device, uint64_t chain, uint32_t* out) {
    return serialized_acquire(id<VkDevice>(device), id<VkSwapchainKHR>(chain), 0, VK_NULL_HANDLE, VK_NULL_HANDLE, out);
}
extern "C" int32_t rsf_fixture_wsi_images(uint64_t device, uint64_t chain, uint32_t* count) {
    return serialized_images(id<VkDevice>(device), id<VkSwapchainKHR>(chain), count, nullptr);
}
extern "C" int32_t rsf_fixture_wsi_present(uint64_t queue) {
    const VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
    return serialized_present(id<VkQueue>(queue), &info);
}
extern "C" int32_t rsf_fixture_wsi_device_proc(uint64_t device) {
    const auto proc = device_proc(id<VkDevice>(device), "vkBindImageMemory2KHR");
    if (!proc) return 0;
    return proc == reinterpret_cast<PFN_vkVoidFunction>(rsf_fixture_wsi_entry("vkGetSwapchainImagesKHR")) ? 1 : -1;
}
extern "C" int32_t rsf_fixture_wsi_graphics_submit(uint64_t fence) {
    return graphics_submit(0, nullptr, id<VkFence>(fence));
}
int main(int argc, char** argv) {
    if (argc != 2) return 1;
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    SetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", nullptr);
    return rsf_fixture_ffx_run(uint32_t(std::strtoul(argv[1], nullptr, 10)));
}
