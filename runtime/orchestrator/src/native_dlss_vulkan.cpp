// SPDX-License-Identifier: MIT
#include "native_dlss_vulkan_platform.h"
#include "../../backends/dlss/include/rescaleframe/dlss_preset.h"
#include <rescaleframe/native_streamline_vulkan.h>
#include <cmath>
#include <cstring>
#include <memory>

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include "vulkan_fg_resources_native.h"
#include "../../vulkan/streamline/sdk_records_checks.h"
#include <sl_dlss.h>
#include <DirectXMath.h>
#include "../../backends/common/frame_capture_vk.h"
namespace {
const sl::ViewportHandle viewport(1);
using Image = rsf_vkfg_image;
template<class T> T entry(void* module, const char* name) { return reinterpret_cast<T>(GetProcAddress(static_cast<HMODULE>(module), name)); }
}
#include "sharpening_vulkan.h"
struct Native {
    rsf_sharpenvk_host sharpen{};
    ~Native() { rsf_sharpenvk_host_destroy(&sharpen); }
    rsf_srvk_setup setup{};
    rsf::vkfg::Device gpu;
    PFun_slSetConstants* constants = nullptr; PFun_slSetTagForFrame* tags = nullptr;
    PFun_slEvaluateFeature* evaluate = nullptr; PFun_slFreeResources* release = nullptr;
    PFun_slDLSSGetOptimalSettings* optimal = nullptr; PFun_slDLSSSetOptions* options = nullptr;
    rsf::FrameCaptureVk capture{rsf::FrameCaptureVk::Completion::command_retirement};
};
namespace {
sl::Resource resource(const Image& image, uint32_t layout) {
    sl::Resource result{sl::ResourceType::eTex2d, reinterpret_cast<void*>(image.image), nullptr,
        reinterpret_cast<void*>(image.view), layout};
    result.width = image.width; result.height = image.height; result.nativeFormat = image.format;
    result.mipLevels = result.arrayLayers = 1;
    result.flags = 0; result.usage = image.usage;
    return result;
}
rsf_backend_result initialize(void* module, uintptr_t, void* user) noexcept try {
    auto& init = *static_cast<rsf_vkdlss_init*>(user);
    auto& self = *static_cast<Native*>(init.native);
    auto supported = entry<PFun_slIsFeatureSupported*>(module, "slIsFeatureSupported");
    auto feature = entry<PFun_slGetFeatureFunction*>(module, "slGetFeatureFunction");
    self.constants = entry<PFun_slSetConstants*>(module, "slSetConstants"); self.tags = entry<PFun_slSetTagForFrame*>(module, "slSetTagForFrame");
    self.evaluate = entry<PFun_slEvaluateFeature*>(module, "slEvaluateFeature"); self.release = entry<PFun_slFreeResources*>(module, "slFreeResources");
    if (!supported || !feature || !self.constants || !self.tags || !self.evaluate || !self.release) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    sl::AdapterInfo adapter{}; adapter.vkPhysicalDevice = self.setup.physical_device;
    if (supported(sl::kFeatureDLSS, adapter) != sl::Result::eOk) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    void* optimal = nullptr; void* options = nullptr;
    if (feature(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", optimal) != sl::Result::eOk ||
        feature(sl::kFeatureDLSS, "slDLSSSetOptions", options) != sl::Result::eOk || !optimal || !options) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self.optimal = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal); self.options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    sl::DLSSOptions request{};
    std::memcpy(static_cast<void*>(&request), init.options, sizeof(request));
    sl::DLSSOptimalSettings plan{};
    if (self.optimal(request, plan) != sl::Result::eOk || !plan.optimalRenderWidth || !plan.optimalRenderHeight) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    init.width = plan.optimalRenderWidth; init.height = plan.optimalRenderHeight;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
int32_t set_constants(void* native, uintptr_t token, const rsf_slvk_sdk_constants* input) {
    auto& self = *static_cast<Native*>(native);
    DirectX::XMFLOAT4X4 previous{}, inverse{}; std::memcpy(&previous, input->clip_to_previous, sizeof(previous));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&previous)));
    const float det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    sl::Constants constants{};
    std::memcpy(static_cast<void*>(&constants), input, sizeof(constants));
    static_assert(sizeof(constants.prevClipToClip) == sizeof(inverse));
    std::memcpy(static_cast<void*>(&constants.prevClipToClip), &inverse, sizeof(inverse));
    auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    return self.constants(constants, frame, viewport) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
int32_t set_options(void* native, const rsf_vkdlss_options* input) {
    auto& self = *static_cast<Native*>(native);
    sl::DLSSOptions options{};
    std::memcpy(static_cast<void*>(&options), input, sizeof(options));
    return self.options(viewport, options) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
int32_t tag(void* native, uintptr_t token, void* commands, const rsf_vkdlss_tag* plan) {
    auto& self = *static_cast<Native*>(native);
    sl::Resource resources[5]{};
    sl::ResourceTag tags[5]{};
    for (size_t i = 0; i < 5; ++i) {
        const auto& item = plan[i];
        const sl::Extent extent{0, 0, item.width, item.height};
        if (item.image) resources[i] = resource(*item.image, item.layout);
        tags[i] = sl::ResourceTag{item.image ? &resources[i] : nullptr, item.kind,
            sl::ResourceLifecycle::eValidUntilEvaluate, &extent};
    }
    return self.tags(*reinterpret_cast<sl::FrameToken*>(token), viewport, tags, 5,
        reinterpret_cast<sl::CommandBuffer*>(commands)) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
void capture_parameters(void*, const rsf_srvk_frame* input, uint32_t reset) {
    rsf_capture_sr_parameters(input->record, RSF_SR_DLSS, input->motion_scale_x,
        input->motion_scale_y, input->pre_exposure, reset);
}

void capture_record(void* native, void* commands, const rsf_srvk_image* image,
    const rsf_frame_record* record, const char* role, uint32_t reset) {
    (void)static_cast<Native*>(native)->capture.record(
        static_cast<VkCommandBuffer>(commands), *image, *record, role, reset);
}
int32_t open(const rsf_srvk_setup* setup, void** out) noexcept try {
    auto self = std::make_unique<Native>();
    self->setup = *setup;
    *out = self.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t prepare(void* native, rsf_vkdlss_commands* commands) noexcept try {
    auto* self = static_cast<Native*>(native);
    const auto* setup = &self->setup;
    if (!self->gpu.load_images(*setup, true)) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
#define RSF_LOAD(member, function) commands->member = setup->device_proc(setup->device, #function); if (!commands->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_LOAD(barrier, vkCmdPipelineBarrier);
    RSF_LOAD(copy, vkCmdCopyImage);
    RSF_LOAD(blit, vkCmdBlitImage);
#undef RSF_LOAD
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    VkPhysicalDeviceMemoryProperties memory{};
    properties(static_cast<VkPhysicalDevice>(setup->physical_device), &memory);
    self->gpu.memory_types(memory);
    self->capture.set_provider(0, RSF_SR_DLSS);
    if (!self->capture.initialize(static_cast<VkDevice>(setup->device), setup->device_proc, memory))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

}

const rsf_vkdlss_platform rsf::dlss_vk::leaves{
    open,
    [](void* native) { delete static_cast<Native*>(native); },
    rsf_slvk_invoke,
    initialize,
    prepare,
    [](void* native, Image* image, uint32_t width, uint32_t height, uint32_t format, uint32_t usage) {
        return rsf_core_vkfg_ensure_image_usage(&rsf::vkfg::platform, static_cast<Native*>(native)->gpu.get(),
            image, width, height, format, usage, 1);
    },
    [](void* native, Image* image) {
        rsf_core_vkfg_release_image(&rsf::vkfg::platform, static_cast<Native*>(native)->gpu.get(), image);
    },
    set_constants, set_options,
    tag,
    [](void* native, uintptr_t token, void* commands) -> int32_t {
        auto& self = *static_cast<Native*>(native);
        const sl::BaseStructure* inputs[] = {&viewport};
        return self.evaluate(sl::kFeatureDLSS, *reinterpret_cast<sl::FrameToken*>(token), inputs, 1,
            reinterpret_cast<sl::CommandBuffer*>(commands)) == sl::Result::eOk ?
            RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    },
    capture_parameters, capture_record,
    [](void* native, void* commands, const rsf_srvk_frame* input, uint64_t output) {
        auto& self = *static_cast<Native*>(native);
        (void)rsf_sharpenvk_host_run(&self.sharpen, &self.setup, commands, input, output);
    },
    [](void*, uintptr_t, void* native) -> int32_t {
        return static_cast<Native*>(native)->release(sl::kFeatureDLSS, viewport) == sl::Result::eOk ?
            RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    },
    [](void* native) {
        auto& self = *static_cast<Native*>(native);
        (void)self.capture.flush();
        rsf_sharpenvk_host_clear(&self.sharpen);
    },
    [](void* native, void* commands) {
        auto& self = *static_cast<Native*>(native);
        rsf_sharpenvk_host_retire(&self.sharpen, commands);
        self.capture.retire_commands(static_cast<VkCommandBuffer>(commands));
    },
    rsf_dlss_preset_valid,
};
static_assert(sizeof(rsf_vkdlss_platform) == 18 * sizeof(void*));
static_assert(sizeof(rsf_vkdlss_tag) == 24);
static_assert(sl::kBufferTypeDepth == 0 && sl::kBufferTypeMotionVectors == 1 &&
    sl::kBufferTypeScalingInputColor == 3 && sl::kBufferTypeScalingOutputColor == 4 &&
    sl::kBufferTypeBiasCurrentColorHint == 29);
static_assert(sizeof(sl::DLSSOptions) == sizeof(rsf_vkdlss_options) &&
    alignof(sl::DLSSOptions) == alignof(rsf_vkdlss_options) &&
    __is_trivially_copyable(sl::DLSSOptions));
static_assert(sl::DLSSOptions::s_structType.data1 == 0x6ac826e4);
static_assert(uint32_t(sl::DLSSMode::eDLAA) == 6 && uint32_t(sl::DLSSMode::eMaxQuality) == 3 &&
    uint32_t(sl::DLSSMode::eBalanced) == 2 && uint32_t(sl::DLSSMode::eMaxPerformance) == 1 &&
    uint32_t(sl::DLSSMode::eUltraPerformance) == 4);
#if defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Winvalid-offsetof"
#endif
#define RSF_SR_OPTION(vendor, wire) \
    static_assert(offsetof(sl::DLSSOptions, vendor) == offsetof(rsf_vkdlss_options, wire))
RSF_SR_OPTION(mode, mode);
RSF_SR_OPTION(outputWidth, width);
RSF_SR_OPTION(outputHeight, height);
RSF_SR_OPTION(preExposure, pre_exposure);
RSF_SR_OPTION(exposureScale, exposure_scale);
RSF_SR_OPTION(colorBuffersHDR, hdr);
RSF_SR_OPTION(indicatorInvertAxisX, invert_x);
RSF_SR_OPTION(indicatorInvertAxisY, invert_y);
RSF_SR_OPTION(dlaaPreset, presets);
static_assert(offsetof(sl::DLSSOptions, ultraQualityPreset) ==
    offsetof(rsf_vkdlss_options, presets) + 5 * sizeof(uint32_t));
RSF_SR_OPTION(useAutoExposure, auto_exposure);
RSF_SR_OPTION(alphaUpscalingEnabled, alpha_upscaling);
#undef RSF_SR_OPTION
#if defined(__GNUC__)
#pragma GCC diagnostic pop
#endif
#endif
