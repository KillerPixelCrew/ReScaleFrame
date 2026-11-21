// SPDX-License-Identifier: MIT
// Shared CPU harness: the real Rust owner drives the real native FFX leaf.
// Only the module, SDK and Vulkan entry points are fakes. No GPU work.
#pragma once
#include "../runtime/orchestrator/src/native_fg_vulkan.cpp"
#include <cstdio>
#include <cstdlib>

extern "C" int32_t rsf_slvk_without_dlssg(int32_t (*call)(void*), void* user) { return call(user); }
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call call, void* user) { return call(user); }
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function call, void* queue, const void* info) {
    return reinterpret_cast<PFN_vkQueuePresentKHR>(call)(static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
}

namespace fake {
inline void require(bool value, const char* text) { if (!value) { std::fprintf(stderr, "%s\n", text); std::exit(1); } }
template<class T> T id(uint64_t value) { return reinterpret_cast<T>(static_cast<uintptr_t>(value)); }

struct State {
    uint64_t next = 100;
    uint32_t resets = 0, submits = 0, waits = 0, copies = 0, sdk_records = 0, generated = 0;
    uint32_t configurations = 0, activations = 0, present_calls = 0, drains = 0, idles = 0, locks = 0, unlocks = 0;
    uint32_t effects_destroyed = 0, swapchains_destroyed = 0, chains_destroyed = 0;
    uint32_t destroyed_images = 0, freed_memories = 0, destroyed_fences = 0;
    bool reject_prepare = false, reject_wait = false, reject_bind = false, reject_drain = false, reject_effect_destroy = false;
    bool reject_swapchain_destroy = false, reject_generate = false, check_reset = false, expected_reset = false, enabled = false;
    uint64_t configured_frame = 0, generated_frame = 0;
    VkResult present_result = VK_SUCCESS;
    FfxApiFrameGenerationDispatchFunc callback = nullptr;
    void* context = nullptr;
    rsf_ffxgvk_native* native = nullptr;
} state;

VkResult VKAPI_PTR create_fence(VkDevice, const VkFenceCreateInfo*, const VkAllocationCallbacks*, VkFence* out) {
    *out = id<VkFence>(++state.next); return VK_SUCCESS;
}
void VKAPI_PTR destroy_fence(VkDevice, VkFence, const VkAllocationCallbacks*) { ++state.destroyed_fences; }
VkResult VKAPI_PTR reset_fences(VkDevice, uint32_t count, const VkFence* fence) {
    require(count == 1 && *fence, "reset has a real completion identity"); ++state.resets; return VK_SUCCESS;
}
VkResult VKAPI_PTR wait_fences(VkDevice, uint32_t count, const VkFence* fence, VkBool32, uint64_t timeout) {
    require(count == 1 && *fence && timeout == 15000000000ull, "wait observes the earlier completion before reset");
    ++state.waits; return state.reject_wait ? VK_TIMEOUT : VK_SUCCESS;
}
int32_t submit(void*, uint32_t count, const void* commands, uint64_t fence) {
    require(!count && !commands && fence, "fences are queued after successful host submissions"); ++state.submits; return VK_SUCCESS;
}
void lock(void*) { ++state.locks; }
void unlock(void*) { ++state.unlocks; }
VkResult VKAPI_PTR create_image(VkDevice, const VkImageCreateInfo*, const VkAllocationCallbacks*, VkImage* out) {
    *out = id<VkImage>(++state.next); return VK_SUCCESS;
}
void VKAPI_PTR destroy_image(VkDevice, VkImage, const VkAllocationCallbacks*) { ++state.destroyed_images; }
void VKAPI_PTR requirements(VkDevice, VkImage, VkMemoryRequirements* out) { out->size = 4096; out->memoryTypeBits = 1; }
VkResult VKAPI_PTR allocate(VkDevice, const VkMemoryAllocateInfo*, const VkAllocationCallbacks*, VkDeviceMemory* out) {
    *out = id<VkDeviceMemory>(++state.next); return VK_SUCCESS;
}
void VKAPI_PTR free_memory(VkDevice, VkDeviceMemory, const VkAllocationCallbacks*) { ++state.freed_memories; }
VkResult VKAPI_PTR bind(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) { return state.reject_bind ? VK_ERROR_UNKNOWN : VK_SUCCESS; }
void VKAPI_PTR barrier(VkCommandBuffer, VkPipelineStageFlags, VkPipelineStageFlags, VkDependencyFlags, uint32_t, const VkMemoryBarrier*,
    uint32_t, const VkBufferMemoryBarrier*, uint32_t, const VkImageMemoryBarrier*) {}
void VKAPI_PTR copy(VkCommandBuffer, VkImage, VkImageLayout, VkImage, VkImageLayout, uint32_t, const VkImageCopy*) { ++state.copies; }
VkResult VKAPI_PTR idle(VkDevice) { ++state.idles; return VK_SUCCESS; }

ffxReturnCode_t dispatch(ffxContext* context, const ffxDispatchDescHeader* header) {
    switch (header->type) {
    case FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE:
        require(context == &state.native->effect, "prepare reaches the effect context");
        ++state.sdk_records; // A failing SDK call can already have recorded GPU work.
        return state.reject_prepare ? FFX_API_RETURN_ERROR_PARAMETER : FFX_API_RETURN_OK;
    case FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION: {
        const auto& request = *reinterpret_cast<const ffxDispatchDescFrameGeneration*>(header);
        if (state.check_reset) require(request.reset == state.expected_reset, "camera reset reaches SDK dispatch without changing activation");
        if (state.reject_generate) return FFX_API_RETURN_ERROR;
        state.generated_frame = request.frameID;
        ++state.generated; return FFX_API_RETURN_OK;
    }
    case FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK:
        require(context == &state.native->swapchain && state.locks == state.unlocks + 1, "presents drain on the swapchain under the queue lock");
        ++state.drains; return state.reject_drain ? FFX_API_RETURN_ERROR : FFX_API_RETURN_OK;
    default:
        require(false, "unexpected SDK dispatch"); return FFX_API_RETURN_ERROR;
    }
}
ffxReturnCode_t configure(ffxContext* context, const ffxConfigureDescHeader* header) {
    const auto& request = *reinterpret_cast<const ffxConfigureDescFrameGeneration*>(header);
    require(context == &state.native->effect && request.header.type == FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION &&
        request.swapChain == state.native->chain && !request.allowAsyncWorkloads, "configure targets the owned presenter");
    const bool enabled = request.frameGenerationEnabled;
    if (enabled) require(request.frameGenerationCallback && request.frameGenerationCallbackUserContext == state.native &&
        request.HUDLessColor.resource, "enabled presenter receives its callback and leased HUD-less input");
    if (state.enabled != enabled) ++state.activations;
    state.enabled = enabled; state.configured_frame = request.frameID;
    state.callback = request.frameGenerationCallback; state.context = request.frameGenerationCallbackUserContext;
    ++state.configurations; return FFX_API_RETURN_OK;
}
ffxReturnCode_t destroy_context(ffxContext* context, const ffxAllocationCallbacks*) {
    if (context == &state.native->effect) {
        if (state.reject_effect_destroy) return FFX_API_RETURN_ERROR;
        ++state.effects_destroyed;
    } else {
        require(context == &state.native->swapchain && !state.native->chain, "swapchain context outlives its chain");
        if (state.reject_swapchain_destroy) return FFX_API_RETURN_ERROR;
        ++state.swapchains_destroyed;
    }
    return FFX_API_RETURN_OK;
}
VkResult VKAPI_PTR present(VkQueue queue, const VkPresentInfoKHR*) {
    require(queue == static_cast<VkQueue>(state.native->setup.queues[0]), "presents through the graphics queue");
    ++state.present_calls;
    if (state.enabled) {
        // FrameInterpolationSwapChainVK calls the generation callback inside queuePresent.
        ffxDispatchDescFrameGeneration request{}; request.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION;
        request.frameID = state.configured_frame + 1;
        require(state.callback(&request, state.context) == FFX_API_RETURN_ERROR_PARAMETER,
            "generation callback refuses a different frame than the configured source");
        request.frameID = state.configured_frame; request.numGeneratedFrames = 1;
        const ffxReturnCode_t expected = state.reject_generate ? FFX_API_RETURN_ERROR : FFX_API_RETURN_OK;
        require(state.callback(&request, state.context) == expected,
            "generation callback returns the SDK result for the leased source frame");
    }
    return state.present_result;
}
VkResult VKAPI_PTR swapchain_images(VkDevice, VkSwapchainKHR chain, uint32_t* count, VkImage* images) {
    require(chain == state.native->chain, "images come from the owned chain");
    // Match the pinned SDK bug: the fill call does not update an oversized count.
    if (!images) *count = 3;
    else for (uint32_t i = 0; i < *count && i < 3; ++i) images[i] = id<VkImage>(1000 + i);
    return *count < 3 ? VK_INCOMPLETE : VK_SUCCESS;
}
VkResult VKAPI_PTR acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t* index) { *index = 1; return VK_SUCCESS; }
void VKAPI_PTR destroy_chain(VkDevice, VkSwapchainKHR chain, const VkAllocationCallbacks*, void* context) {
    require(chain && context == state.native->swapchain, "chain destroyed with its swapchain context"); ++state.chains_destroyed;
}
uint64_t present_count(VkSwapchainKHR) { return state.present_calls; }

// The leaf the Rust WSI route lends to a hook on the fixture device, or null.
inline rsf_ffxgvk_native* routed() {
    auto* native = rsf_core_ffxgvk_wsi_borrow(id<void*>(1), 1);
    if (native) rsf_core_ffxgvk_wsi_return();
    return native;
}

// The fake open replaces module loading, Vulkan entry resolution and WSI
// registration. Admission is the real Rust route. Every later native step is
// the production leaf.
int32_t open(const rsf_fgvk_setup* setup, uint32_t width, uint32_t height, rsf_ffxgvk_native** out, void** resources) {
    require(!routed(), "Rust publishes the leaf only after open");
    auto* self = new rsf_ffxgvk_native{};
    self->setup = *setup; self->width = width; self->height = height;
    self->create = nullptr; self->destroy = destroy_context; self->dispatch = dispatch; self->configure = configure;
    self->barrier = barrier; self->copy = copy; self->idle = idle;
    auto& gpu = self->gpu;
    gpu.device = static_cast<VkDevice>(setup->graphics.device); gpu.graphics_submit = setup->graphics_submit; gpu.queue_user = setup->queue_user;
    gpu.create_fence = create_fence; gpu.destroy_fence = destroy_fence; gpu.reset_fences = reset_fences; gpu.wait_fences = wait_fences;
    gpu.create_image = create_image; gpu.destroy_image = destroy_image; gpu.requirements = requirements; gpu.allocate = allocate;
    gpu.free_memory = free_memory; gpu.bind = bind; gpu.local_types = 1;
    state.native = self; *resources = &self->gpu; *out = self;
    return RSF_BACKEND_OK;
}
int32_t create_swapchain(rsf_ffxgvk_native* self, uint64_t* chain) {
    self->swapchain = id<ffxContext>(4); self->chain = id<VkSwapchainKHR>(3); *chain = 3; return RSF_BACKEND_OK;
}
int32_t query_swapchain(rsf_ffxgvk_native* self) {
    auto& functions = self->functions;
    functions.pOutQueuePresentKHR = present; functions.pOutGetSwapchainImagesKHR = swapchain_images;
    functions.pOutAcquireNextImageKHR = acquire; functions.pOutDestroySwapchainFFXAPI = destroy_chain;
    functions.pOutGetLastPresentCountFFXAPI = present_count;
    return RSF_BACKEND_OK;
}
int32_t create_effect(rsf_ffxgvk_native* self) { self->effect = id<ffxContext>(5); return RSF_BACKEND_OK; }
int32_t query_provider(rsf_ffxgvk_native*) { return RSF_BACKEND_OK; }

inline rsf_ffxgvk_ops ops() {
    auto value = operations;
    value.open = open; value.create_swapchain = create_swapchain; value.query_swapchain = query_swapchain;
    value.create_effect = create_effect; value.query_provider = query_provider;
    return value;
}

struct Setup {
    VkSwapchainCreateInfoKHR info{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
    rsf_fgvk_setup value{sizeof(value), RSF_FGVK_ABI_VERSION};
    rsf_ffxgvk_swapchain swapchain{640, 360, VK_FORMAT_R8G8B8A8_UNORM, VK_COLOR_SPACE_SRGB_NONLINEAR_KHR, 1, 1, 0};
    Setup() {
        auto& graphics = value.graphics;
        graphics.struct_size = sizeof(graphics); graphics.abi_version = RSF_SRVK_ABI_VERSION;
        graphics.instance = id<void*>(7); graphics.physical_device = id<void*>(8); graphics.device = id<void*>(1);
        graphics.instance_proc = [](void*, const char*) -> rsf_vk_function { return nullptr; };
        graphics.device_proc = graphics.instance_proc; graphics.runtime_directory_utf8 = "C:\\fake";
        graphics.depth_inverted = 1;
        for (uint32_t i = 0; i < 4; ++i) value.queues[i] = id<void*>(20 + i);
        value.create_info = &info; value.graphics_submit = submit; value.lock_queues = lock; value.unlock_queues = unlock;
    }
};

inline rsf_frame_record record(uint64_t session, uint64_t frame, uint32_t view = 0, uint32_t generation = 1, uint32_t flags = 0) {
    rsf_frame_record result{}; result.struct_size = sizeof(result); result.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    result.session_id = session; result.frame_id = frame; result.view_id = view; result.resource_generation = generation; result.flags = flags;
    result.render_width = result.output_width = 640; result.render_height = result.output_height = 360; result.frame_time_ms = 16.667f;
    auto& camera = result.camera; camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = camera.output_width = 640; camera.render_height = camera.output_height = 360;
    camera.near_plane = 0.05f; camera.far_plane = 100; camera.vertical_fov_radians = 1; camera.depth_inverted = 1;
    for (auto* matrix : {camera.view_to_clip, camera.clip_to_view, camera.view_to_world, camera.world_to_view, camera.clip_to_previous_clip})
        for (uint32_t i = 0; i < 16; i += 5) matrix[i] = 1;
    return result;
}
inline rsf_fgvk_frame frame(const rsf_frame_record& source) {
    rsf_fgvk_frame result{}; result.struct_size = sizeof(result); result.abi_version = RSF_FGVK_ABI_VERSION; result.record = &source;
    auto image = [](uint32_t format) { return rsf_srvk_image{id<void*>(1), 12, format, 640, 360, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1}; };
    result.depth = image(VK_FORMAT_R32_SFLOAT); result.motion = image(VK_FORMAT_R16G16_SFLOAT); result.hudless = image(VK_FORMAT_R8G8B8A8_UNORM);
    result.motion_scale_x = 640; result.motion_scale_y = 360; return result;
}
inline void complete(rsf_ffxgvk* self, const rsf_frame_record& source, void* prepare, void* hudless) {
    auto input = frame(source);
    require(rsf_ffxgvk_prepare(self, prepare, &input) == RSF_BACKEND_OK && rsf_ffxgvk_hudless(self, hudless, &input) == RSF_BACKEND_OK,
        "real prepare and HUD-less recording accepted the source");
}
inline rsf_ffxgvk* start(const rsf_ffxgvk_ops& table, Setup& setup) {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr); SetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", nullptr);
    rsf_ffxgvk* self = nullptr; uint64_t chain = 0;
    require(rsf_core_ffxgvk_create(&table, &setup.value, &setup.swapchain, &self, &chain) == RSF_BACKEND_OK && self && chain == 3,
        "Rust owner admits the swapchain and opens the native leaf");
    return self;
}
}
