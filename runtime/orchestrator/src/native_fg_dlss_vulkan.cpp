// SPDX-License-Identifier: MIT
#include "native_fg_dlss_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include <sl.h>
#include <sl_dlss_g.h>
#include <DirectXMath.h>
#include <cmath>
#include <cstring>
#include <limits>
#include <memory>
#include "native_fg_dlss_vulkan_capture.h"
#include "vulkan_fg_copy.h"
#include "vulkan_fg_resources_native.h"
#include "../../backends/common/frame_capture_vk.h"

extern "C" uint64_t VulkanStorySlWorkerTimeoutEpoch();
namespace {
template<class T> T entry(void* module, const char* name) { return reinterpret_cast<T>(GetProcAddress(static_cast<HMODULE>(module), name)); }
sl::float4x4 matrix(const float* values) {
    sl::float4x4 result{};
    for (uint32_t row = 0; row < 4; ++row) for (uint32_t column = 0; column < 4; ++column)
        (&result[row].x)[column] = values[row * 4 + column];
    return result;
}

using Image = rsf_vkfg_image;
const sl::ViewportHandle viewport(0);
void on_error(const sl::APIError& error) { rsf_core_dlgvk_report_api_error(static_cast<int32_t>(error.vkRes)); }

struct Native {
    rsf_fgvk_setup setup{};
    VkSwapchainCreateInfoKHR info{};
    rsf::vkfg::Device gpu;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdCopyImage copy = nullptr;
    PFun_slSetConstants* constants = nullptr;
    PFun_slSetTagForFrame* tags = nullptr;
    PFun_slFreeResources* release = nullptr;
    PFun_slDLSSGSetOptions* options = nullptr;
    PFun_slDLSSGGetState* state = nullptr;
    rsf::FrameCaptureVk capture;
    rsf::DlssPresentCapture present_capture;

};

static_assert(sizeof(rsf_dlgvk_sample) == 40);
static_assert(sizeof(rsf_dlgvk_sdk_call) == 96);
static_assert(sizeof(rsf_dlgvk_chain_support) == 8);
static_assert(sizeof(rsf_dlgvk_platform) == 160);
static_assert(VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_IMAGE_USAGE_TRANSFER_DST_BIT == 2 &&
    VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44);

rsf_backend_result sdk_result(rsf_dlgvk_sdk_call& call, sl::Result result) {
    call.result = static_cast<int32_t>(result);
    return result == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result initialize(void* module, uintptr_t, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    auto& self = *static_cast<Native*>(call.native);
    auto supported = entry<PFun_slIsFeatureSupported*>(module, "slIsFeatureSupported");
    auto feature = entry<PFun_slGetFeatureFunction*>(module, "slGetFeatureFunction");
    self.constants = entry<PFun_slSetConstants*>(module, "slSetConstants");
    self.tags = entry<PFun_slSetTagForFrame*>(module, "slSetTagForFrame");
    self.release = entry<PFun_slFreeResources*>(module, "slFreeResources");
    if (!supported || !feature || !self.constants || !self.tags || !self.release) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    sl::AdapterInfo adapter{}; adapter.vkPhysicalDevice = self.setup.graphics.physical_device;
    const auto support = supported(sl::kFeatureDLSS_G, adapter);
    call.result = static_cast<int32_t>(support);
    if (support != sl::Result::eOk) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    void* options = nullptr; void* state = nullptr;
    if (feature(sl::kFeatureDLSS_G, "slDLSSGSetOptions", options) != sl::Result::eOk ||
        feature(sl::kFeatureDLSS_G, "slDLSSGGetState", state) != sl::Result::eOk || !options || !state)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self.options = reinterpret_cast<PFun_slDLSSGSetOptions*>(options);
    self.state = reinterpret_cast<PFun_slDLSSGGetState*>(state);
    return RSF_BACKEND_OK;
}
rsf_backend_result configure(void*, uintptr_t, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    const auto& self = *static_cast<Native*>(call.native);
    const bool enabled = call.count != 0;
    // Options apply to the next proxy Present. A new owner must publish Off
    // too, since its local state cannot describe the shared SDK viewport.
    sl::DLSSGOptions options{}; options.mode = enabled ? sl::DLSSGMode::eOn : sl::DLSSGMode::eOff;
    options.flags = sl::DLSSGFlags::eRetainResourcesWhenOff;
    options.numFramesToGenerate = enabled ? call.count : 1;
    options.colorWidth = self.info.imageExtent.width; options.colorHeight = self.info.imageExtent.height;
    options.colorBufferFormat = self.info.imageFormat; options.numBackBuffers = call.backbuffers;
    options.depthBufferFormat = VK_FORMAT_R32_SFLOAT; options.mvecBufferFormat = VK_FORMAT_R16G16_SFLOAT;
    options.hudLessBufferFormat = VK_FORMAT_R8G8B8A8_UNORM;
    options.queueParallelismMode = sl::DLSSGQueueParallelismMode::eBlockNoClientQueues;
    // No UI alpha/color producer is supplied by this ABI. HUD-less color alone
    // supports the SDK's ordinary UI detection, not separate UI recomposition.
    options.enableUserInterfaceRecomposition = sl::Boolean::eFalse;
    options.onErrorCallback = on_error;
    return sdk_result(call, self.options(viewport, options));
}

rsf_backend_result sample(void*, uintptr_t, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    const auto& self = *static_cast<Native*>(call.native);
    sl::DLSSGState state{};
    const auto result = sdk_result(call, self.state(viewport, state, nullptr));
    if (result == RSF_BACKEND_OK) {
        call.sample = {reinterpret_cast<uint64_t>(state.inputsProcessingCompletionFence),
            state.lastPresentInputsProcessingCompletionFenceValue, state.numFramesActuallyPresented,
            state.numFramesToGenerateMax, static_cast<uint32_t>(state.status), state.minWidthOrHeight,
            state.bIsVsyncSupportAvailable == sl::Boolean::eTrue, state.bIsDynamicMFGSupported == sl::Boolean::eTrue};
    }
    return result;
}
sl::Resource resource(const Image& image) {
    sl::Resource result{sl::ResourceType::eTex2d, reinterpret_cast<void*>(image.image), nullptr,
        reinterpret_cast<void*>(image.view), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL};
    result.width = image.width; result.height = image.height; result.nativeFormat = image.format;
    result.mipLevels = result.arrayLayers = 1;
    result.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    return result;
}
rsf_backend_result constants(void*, uintptr_t token, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    const auto& self = *static_cast<Native*>(call.native);
    const auto& input = *call.input; const auto& record = *call.record; const auto& camera = record.camera;
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    DirectX::XMFLOAT4X4 previous{}, inverse{}; std::memcpy(&previous, camera.clip_to_previous_clip, sizeof(previous));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&previous)));
    const auto det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    sl::Constants constants{}; constants.cameraViewToClip = matrix(camera.view_to_clip); constants.clipToCameraView = matrix(camera.clip_to_view);
    constants.clipToPrevClip = matrix(camera.clip_to_previous_clip); constants.prevClipToClip = matrix(&inverse.m[0][0]);
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {-camera.view_to_world[8], -camera.view_to_world[9], -camera.view_to_world[10]};
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane > 0 ? camera.far_plane : std::numeric_limits<float>::max();
    constants.cameraFOV = camera.vertical_fov_radians; constants.cameraAspectRatio = float(self.info.imageExtent.width) / self.info.imageExtent.height;
    constants.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]}; constants.cameraPinholeOffset = {0, 0};
    // The adapter's scale already accounts for the engine motion convention.
    // As in DLSS SR, convert its pixel scale to normalized texture coordinates.
    constants.mvecScale = {input.motion_scale_x / record.render_width, input.motion_scale_y / record.render_height};
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue; constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = camera.motion_jittered ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max();
    constants.reset = (record.flags & RSF_FRAME_FLAG_RESET) ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    return sdk_result(call, self.constants(constants, frame, viewport));
}
rsf_backend_result tag(void*, uintptr_t token, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    const auto& self = *static_cast<Native*>(call.native);
    const auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    auto* commands = reinterpret_cast<sl::CommandBuffer*>(call.commands);
    auto first = resource(call.images[0]);
    // Engine guides are overwritten before Present. Streamline copies the tags
    // on this recording list while the original CPU token is leased.
    if (call.count == 2) {
        auto motion = resource(call.images[1]);
        const sl::Extent render{0, 0, call.record->render_width, call.record->render_height};
        sl::ResourceTag tags[] = {{&first, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow, &render},
            {&motion, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eOnlyValidNow, &render}};
        return sdk_result(call, self.tags(frame, viewport, tags, 2, commands));
    }
    const sl::Extent extent{0, 0, self.info.imageExtent.width, self.info.imageExtent.height};
    sl::ResourceTag tags[] = {{&first, sl::kBufferTypeHUDLessColor, sl::ResourceLifecycle::eOnlyValidNow, &extent},
        {nullptr, sl::kBufferTypeUIColorAndAlpha, sl::ResourceLifecycle::eValidUntilPresent, nullptr}};
    return sdk_result(call, self.tags(frame, viewport, tags, 2, commands));
}
rsf_backend_result release(void*, uintptr_t, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    return sdk_result(call, static_cast<Native*>(call.native)->release(sl::kFeatureDLSS_G, viewport));
}

rsf_backend_result open(const rsf_fgvk_setup* setup, void** out) noexcept try {
    auto self = std::make_unique<Native>();
    self->setup = *setup;
    self->info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
    *out = self.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result prepare_chain(void* user, rsf_dlgvk_chain_support* support, void** gpu) noexcept try {
    auto* self = static_cast<Native*>(user);
    const auto& graphics = self->setup.graphics;
#define RSF_LOAD(member, function) self->member = reinterpret_cast<PFN_##function>(graphics.device_proc(graphics.device, #function)); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_LOAD(barrier, vkCmdPipelineBarrier); RSF_LOAD(copy, vkCmdCopyImage);
#undef RSF_LOAD
    auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    VkPhysicalDeviceMemoryProperties memory{};
    properties(static_cast<VkPhysicalDevice>(graphics.physical_device), &memory);
    if (!self->gpu.load(self->setup, true)) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self->gpu.memory_types(memory);
    self->capture.set_provider(0, RSF_SR_DLSS, true);
    if (!self->capture.initialize(static_cast<VkDevice>(graphics.device), graphics.device_proc, memory)) return RSF_BACKEND_ERROR_INIT_FAILED;
    const auto capabilities = reinterpret_cast<PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceSurfaceCapabilitiesKHR"));
    VkSurfaceCapabilitiesKHR supported{};
    if (!capabilities || capabilities(static_cast<VkPhysicalDevice>(graphics.physical_device), self->info.surface, &supported) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    *support = {supported.supportedUsageFlags, self->capture.enabled() ? 1u : 0u};
    *gpu = &self->gpu;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

constexpr rsf_dlgvk_platform platform{
    &rsf::vkfg::platform, open,
    [](void* user) { delete static_cast<Native*>(user); },
    prepare_chain,
    [](void* user) noexcept -> rsf_backend_result {
        try {
            auto& self = *static_cast<Native*>(user);
            return self.present_capture.initialize(self.setup);
        } catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
    },
    rsf_slvk_invoke, initialize, configure, sample, constants, tag, release,
    [](void* user, void* commands, const rsf_vkfg_barrier* plan) {
        rsf::vkfg::record_barrier(static_cast<Native*>(user)->barrier, static_cast<VkCommandBuffer>(commands), *plan);
    },
    [](void* user, void* commands, const rsf_vkfg_image_copy* plan) {
        rsf::vkfg::record_copy(static_cast<Native*>(user)->copy, static_cast<VkCommandBuffer>(commands), *plan);
    },
    [](void* user, void* commands, const rsf_srvk_image* image, const rsf_frame_record* record, uint32_t role, uint64_t group) {
        static constexpr const char* roles[] = {"fg-depth", "fg-motion-uv", "fg-hudless-color"};
        auto& self = *static_cast<Native*>(user);
        (void)self.capture.record(static_cast<VkCommandBuffer>(commands), *image, *record, roles[role],
            record->flags & RSF_FRAME_FLAG_RESET, true, nullptr, group);
    },
    [](void* user, const rsf_frame_record* record, const rsf_dlssgvk_status* status, uint32_t configured) noexcept {
        try {
            static_cast<Native*>(user)->present_capture.remember(*record, status->requested_generated_frames, *status, configured != 0);
        } catch (...) {}
    },
    [](void* user, uint64_t group, uint32_t complete) noexcept {
        try {
            auto& capture = static_cast<Native*>(user)->capture;
            if (complete) capture.complete_group(group); else capture.discard_group(group);
        } catch (...) {}
    },
    [](void* user) noexcept {
        auto& self = *static_cast<Native*>(user);
        (void)self.capture.flush();
        try { self.present_capture.flush(); } catch (...) {}
    },
    [](void* user) noexcept -> rsf_backend_result {
        try { return static_cast<Native*>(user)->present_capture.detach(); }
        catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
    },
    VulkanStorySlWorkerTimeoutEpoch,
};
}
#endif

extern "C" rsf_backend_result rsf_dlssgvk_create(const rsf_fgvk_setup* setup, rsf_dlssgvk** out, uint64_t* chain) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    return rsf_core_dlgvk_create(&platform, setup, out, chain);
#else
    return rsf_core_dlgvk_create(nullptr, setup, out, chain);
#endif
}
extern "C" int32_t rsf_dlssgvk_images(rsf_dlssgvk* owner, uint32_t* count, uint64_t* images) {
    return rsf_core_dlgvk_images(owner, count, images);
}
extern "C" int32_t rsf_dlssgvk_acquire(rsf_dlssgvk* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_core_dlgvk_acquire(owner, timeout, semaphore, fence, index);
}
extern "C" rsf_backend_result rsf_dlssgvk_image_layout(const rsf_dlssgvk* owner, uint32_t* layout) {
    return rsf_core_dlgvk_image_layout(owner, layout);
}
extern "C" rsf_backend_result rsf_dlssgvk_enable(rsf_dlssgvk* owner, uint32_t enabled, uint32_t count) {
    return rsf_core_dlgvk_enable(owner, enabled, count);
}
extern "C" rsf_backend_result rsf_dlssgvk_prepare(rsf_dlssgvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_dlgvk_record(owner, commands, frame, 0);
}
extern "C" rsf_backend_result rsf_dlssgvk_hudless(rsf_dlssgvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_dlgvk_record(owner, commands, frame, 1);
}
extern "C" void rsf_dlssgvk_submitted(rsf_dlssgvk* owner, void* const* commands, uint32_t count) {
    rsf_core_dlgvk_submitted(owner, commands, count);
}
extern "C" int32_t rsf_dlssgvk_present(rsf_dlssgvk* owner, const void* info) {
    return rsf_core_dlgvk_present(owner, info, nullptr, 0);
}
extern "C" int32_t rsf_dlssgvk_present_source(rsf_dlssgvk* owner, const void* info, const rsf_frame_record* source) {
    return rsf_core_dlgvk_present(owner, info, source, 1);
}
extern "C" rsf_backend_result rsf_dlssgvk_get_status(rsf_dlssgvk* owner, rsf_dlssgvk_status* status) {
    return rsf_core_dlgvk_status(owner, status);
}
extern "C" rsf_backend_result rsf_dlssgvk_drain(rsf_dlssgvk* owner) {
    return rsf_core_dlgvk_drain(owner);
}
extern "C" rsf_backend_result rsf_dlssgvk_destroy(rsf_dlssgvk* owner) {
    return rsf_core_dlgvk_destroy(owner);
}
