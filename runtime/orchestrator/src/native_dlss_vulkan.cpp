// SPDX-License-Identifier: MIT
#include "native_dlss_vulkan_platform.h"
#include "../../backends/dlss/include/rescaleframe/dlss_preset.h"
#include <rescaleframe/native_streamline_vulkan.h>
#include <cmath>
#include <cstring>
#include <memory>
#include <limits>

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include "vulkan_fg_resources_native.h"
#include <sl.h>
#include <sl_dlss.h>
#include <DirectXMath.h>
#include "../../backends/common/frame_capture_vk.h"
namespace {
const sl::ViewportHandle viewport(1);
using Image = rsf_vkfg_image;
sl::DLSSMode mode(uint32_t quality) {
    constexpr sl::DLSSMode modes[] = {sl::DLSSMode::eDLAA, sl::DLSSMode::eMaxQuality, sl::DLSSMode::eBalanced,
        sl::DLSSMode::eMaxPerformance, sl::DLSSMode::eUltraPerformance};
    return modes[quality];
}
sl::float4x4 matrix(const float* values) {
    sl::float4x4 result{};
    for (uint32_t row = 0; row < 4; ++row) for (uint32_t column = 0; column < 4; ++column)
        (&result[row].x)[column] = values[row * 4 + column];
    return result;
}
template<class T> T entry(void* module, const char* name) { return reinterpret_cast<T>(GetProcAddress(static_cast<HMODULE>(module), name)); }
}
#include "sharpening_vulkan.h"
struct Native {
    rsf::sharpening::Vulkan sharpen;
    rsf_srvk_setup setup{};
    rsf::vkfg::Device gpu;
    PFun_slSetConstants* constants = nullptr; PFun_slSetTagForFrame* tags = nullptr;
    PFun_slEvaluateFeature* evaluate = nullptr; PFun_slFreeResources* release = nullptr;
    PFun_slDLSSGetOptimalSettings* optimal = nullptr; PFun_slDLSSSetOptions* options = nullptr;
    rsf::FrameCaptureVk capture{rsf::FrameCaptureVk::Completion::command_retirement};
};
namespace {
sl::Resource resource(const Image& image, bool output = false) {
    sl::Resource result{sl::ResourceType::eTex2d, reinterpret_cast<void*>(image.image), nullptr,
        reinterpret_cast<void*>(image.view), uint32_t(output ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL)};
    result.width = image.width; result.height = image.height; result.nativeFormat = image.format;
    result.mipLevels = result.arrayLayers = 1;
    result.flags = 0; result.usage = image.usage;
    return result;
}
struct Init {
    Native& native;
    uint32_t& width;
    uint32_t& height;
};

rsf_backend_result initialize(void* module, uintptr_t, void* user) {
    auto& init = *static_cast<Init*>(user);
    auto& self = init.native;
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
    sl::DLSSOptions request{}; request.mode = mode(self.setup.quality);
    request.outputWidth = self.setup.output_width; request.outputHeight = self.setup.output_height;
    sl::DLSSOptimalSettings plan{};
    if (self.optimal(request, plan) != sl::Result::eOk || !plan.optimalRenderWidth || !plan.optimalRenderHeight) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    init.width = plan.optimalRenderWidth; init.height = plan.optimalRenderHeight;
    return RSF_BACKEND_OK;
}
int32_t set_constants(void* native, uintptr_t token, const rsf_srvk_frame* input, uint32_t reset) {
    auto& self = *static_cast<Native*>(native);
    const auto& record = *input->record; const auto& camera = record.camera;
    DirectX::XMFLOAT4X4 previous{}, inverse{}; std::memcpy(&previous, camera.clip_to_previous_clip, sizeof(previous));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&previous)));
    const float det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    sl::Constants constants{}; constants.cameraViewToClip = matrix(camera.view_to_clip); constants.clipToCameraView = matrix(camera.clip_to_view);
    constants.clipToPrevClip = matrix(camera.clip_to_previous_clip); constants.prevClipToClip = matrix(&inverse.m[0][0]);
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {-camera.view_to_world[8], -camera.view_to_world[9], -camera.view_to_world[10]};
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane > 0 ? camera.far_plane : std::numeric_limits<float>::max();
    constants.cameraFOV = camera.vertical_fov_radians; constants.cameraAspectRatio = float(record.output_width) / record.output_height;
    constants.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]}; constants.cameraPinholeOffset = {0, 0};
    constants.mvecScale = {input->motion_scale_x / record.render_width, input->motion_scale_y / record.render_height};
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue; constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = camera.motion_jittered ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max(); constants.reset = reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    return self.constants(constants, frame, viewport) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
int32_t set_options(void* native, const rsf_srvk_frame* input, uint32_t preset_hint) {
    auto& self = *static_cast<Native*>(native);
    const auto& record = *input->record;
    sl::DLSSOptions options{}; options.mode = mode(self.setup.quality); options.outputWidth = record.output_width; options.outputHeight = record.output_height;
    options.colorBuffersHDR = sl::Boolean::eTrue; options.useAutoExposure = sl::Boolean::eTrue; options.preExposure = input->pre_exposure;
    const auto preset = static_cast<sl::DLSSPreset>(preset_hint);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    return self.options(viewport, options) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
int32_t tag(void* native, uintptr_t token, void* commands, const rsf_srvk_frame* input, const rsf_vkdlss_images* images) {
    auto& self = *static_cast<Native*>(native);
    const auto& record = *input->record;
    sl::Resource color = resource(images->color);
    sl::Resource depth = resource(images->depth);
    sl::Resource motion = resource(images->motion);
    sl::Resource output = resource(images->output, true);
    sl::Resource bias{};
    if (input->bias_current_color.image) bias = resource(images->bias);
    const sl::Extent render{0, 0, images->color.width, images->color.height},
        presented{0, 0, record.output_width, record.output_height};
    sl::ResourceTag tags[] = {{&color, sl::kBufferTypeScalingInputColor, sl::ResourceLifecycle::eValidUntilEvaluate, &render},
        {&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eValidUntilEvaluate, &render},
        {&motion, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eValidUntilEvaluate, &render},
        {&output, sl::kBufferTypeScalingOutputColor, sl::ResourceLifecycle::eValidUntilEvaluate, &presented},
        // An explicit null tag clears a mask used on an earlier frame.
        {input->bias_current_color.image ? &bias : nullptr, sl::kBufferTypeBiasCurrentColorHint, sl::ResourceLifecycle::eValidUntilEvaluate, &render}};
    return self.tags(*reinterpret_cast<sl::FrameToken*>(token), viewport, tags, 5,
        reinterpret_cast<sl::CommandBuffer*>(commands)) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
void capture_parameters(void*, const rsf_srvk_frame* input, uint32_t reset) {
    rsf::capture_sr_parameters(*input->record, RSF_SR_DLSS, input->motion_scale_x,
        input->motion_scale_y, input->pre_exposure, reset);
}

void capture_record(void* native, void* commands, const rsf_srvk_image* image,
    const rsf_frame_record* record, const char* role, uint32_t reset) {
    (void)static_cast<Native*>(native)->capture.record(
        static_cast<VkCommandBuffer>(commands), *image, *record, role, reset);
}
int32_t open(const rsf_srvk_setup* setup, void** out, uint32_t* width, uint32_t* height, rsf_vkdlss_commands* commands) noexcept try {
    auto self = std::make_unique<Native>();
    self->setup = *setup;
    Init init{*self, *width, *height};
    const rsf_slvk_call call{sizeof(call), RSF_SLVK_ABI_VERSION, 0, 0, initialize, &init};
    const auto result = rsf_slvk_invoke(&call);
    if (result != RSF_BACKEND_OK) return result;
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
    *out = self.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

}

const rsf_vkdlss_platform rsf::dlss_vk::leaves{
    open,
    [](void* native) { delete static_cast<Native*>(native); },
    rsf_slvk_invoke,
    [](void* native, Image* image, uint32_t width, uint32_t height, uint32_t format, uint32_t usage) {
        return rsf_core_vkfg_ensure_image_usage(&rsf::vkfg::platform, &static_cast<Native*>(native)->gpu,
            image, width, height, format, usage, 1);
    },
    [](void* native, Image* image) {
        rsf_core_vkfg_release_image(&rsf::vkfg::platform, &static_cast<Native*>(native)->gpu, image);
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
        (void)self.sharpen.run(self.setup, static_cast<VkCommandBuffer>(commands), *input, reinterpret_cast<VkImage>(output));
    },
    [](void*, uintptr_t, void* native) -> int32_t {
        return static_cast<Native*>(native)->release(sl::kFeatureDLSS, viewport) == sl::Result::eOk ?
            RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    },
    [](void* native) {
        auto& self = *static_cast<Native*>(native);
        (void)self.capture.flush();
        self.sharpen.clear();
    },
    [](void* native, void* commands) {
        auto& self = *static_cast<Native*>(native);
        self.sharpen.retire(commands);
        self.capture.retire_commands(static_cast<VkCommandBuffer>(commands));
    },
    rsf_dlss_preset_valid,
};
static_assert(sizeof(rsf_vkdlss_platform) == 16 * sizeof(void*));
#endif
