// SPDX-License-Identifier: MIT
#include "native_fg_dlss_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include <sl.h>
#include "../../vulkan/streamline/sdk_fg_options_checks.h"
#include "../../vulkan/streamline/sdk_records_checks.h"
#include <DirectXMath.h>
#include <cmath>
#include <cstring>
#include <memory>
#include "native_fg_dlss_vulkan_capture.h"
#include "vulkan_fg_copy.h"
#include "vulkan_fg_resources_native.h"
#include "../../backends/common/frame_capture_vk.h"

extern "C" uint64_t VulkanStorySlWorkerTimeoutEpoch();
namespace {
template<class T> T entry(void* module, const char* name) { return reinterpret_cast<T>(GetProcAddress(static_cast<HMODULE>(module), name)); }
using Image = rsf_vkfg_image;
const sl::ViewportHandle viewport(0);
void on_error(const sl::APIError& error) { rsf_core_dlgvk_report_api_error(static_cast<int32_t>(error.vkRes)); }

struct Native {
    rsf_fgvk_setup setup{};
    VkSurfaceKHR surface{};
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
static_assert(sizeof(rsf_dlgvk_tag) == 24);
static_assert(sizeof(rsf_dlgvk_sdk_call) == 96);
static_assert(offsetof(rsf_dlgvk_sdk_call, camera) == 64);
static_assert(offsetof(rsf_dlgvk_sdk_call, tags) == 80);
static_assert(offsetof(rsf_dlgvk_sdk_call, options) == 88);
static_assert(sizeof(rsf_dlgvk_chain_support) == 8);
static_assert(sizeof(rsf_dlgvk_platform) == 160);
static_assert(VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_IMAGE_USAGE_TRANSFER_DST_BIT == 2 &&
    VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44);
static_assert(sl::kBufferTypeDepth == 0 && sl::kBufferTypeMotionVectors == 1 &&
    sl::kBufferTypeHUDLessColor == 2 && sl::kBufferTypeUIColorAndAlpha == 23);
static_assert(sl::ResourceLifecycle::eOnlyValidNow == 0 && sl::ResourceLifecycle::eValidUntilPresent == 1);

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
    sl::DLSSGOptions options{};
    std::memcpy(static_cast<void*>(&options), call.options, sizeof(options));
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
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    DirectX::XMFLOAT4X4 previous{}, inverse{}; std::memcpy(&previous, call.camera->clip_to_previous, sizeof(previous));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&previous)));
    const auto det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    sl::Constants constants{};
    std::memcpy(static_cast<void*>(&constants), call.camera, sizeof(constants));
    static_assert(sizeof(constants.prevClipToClip) == sizeof(inverse));
    std::memcpy(static_cast<void*>(&constants.prevClipToClip), &inverse, sizeof(inverse));
    auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    return sdk_result(call, self.constants(constants, frame, viewport));
}
rsf_backend_result tag(void*, uintptr_t token, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    const auto& self = *static_cast<Native*>(call.native);
    const auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    auto* commands = reinterpret_cast<sl::CommandBuffer*>(call.commands);
    sl::Resource resources[2]{};
    sl::ResourceTag tags[2]{};
    for (size_t i = 0; i < 2; ++i) {
        const auto& plan = call.tags[i];
        const sl::Extent extent{0, 0, plan.width, plan.height};
        if (plan.image) resources[i] = resource(*plan.image);
        tags[i] = sl::ResourceTag{
            plan.image ? &resources[i] : nullptr, plan.kind,
            static_cast<sl::ResourceLifecycle>(plan.lifetime), plan.image ? &extent : nullptr};
    }
    return sdk_result(call, self.tags(frame, viewport, tags, 2, commands));
}
rsf_backend_result release(void*, uintptr_t, void* user) {
    auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    return sdk_result(call, static_cast<Native*>(call.native)->release(sl::kFeatureDLSS_G, viewport));
}

rsf_backend_result open(const rsf_fgvk_setup* setup, void** out) noexcept try {
    auto self = std::make_unique<Native>();
    self->setup = *setup;
    self->surface = static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info)->surface;
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
    if (!capabilities || capabilities(static_cast<VkPhysicalDevice>(graphics.physical_device), self->surface, &supported) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    *support = {supported.supportedUsageFlags, self->capture.enabled() ? 1u : 0u};
    *gpu = self->gpu.get();
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
        rsf_core_vkfg_record_barrier(static_cast<Native*>(user)->barrier, static_cast<VkCommandBuffer>(commands), plan);
    },
    [](void* user, void* commands, const rsf_vkfg_image_copy* plan) {
        rsf_core_vkfg_record_copy(static_cast<Native*>(user)->copy, static_cast<VkCommandBuffer>(commands), plan);
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
