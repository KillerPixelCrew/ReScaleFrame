// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/dlss_native12.h>
#include "streamline_frame.h"
#include "streamline_load.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstring>
#include <cmath>
#include <limits>
#include <cstdio>
#include <memory>
#include <string>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#include <sl_dlss.h>
#include <sl_consts.h>
#include <DirectXMath.h>
#endif

struct rsf_dlss_native12 {
    HMODULE module = nullptr;
    bool initialized = false;
    bool borrowed_engine = false;
    bool sdr = false;
    uint32_t preset = 0;
    rsf_streamline_host* shared_host = nullptr;
    rsf_quality quality = RSF_QUALITY_QUALITY;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    std::shared_ptr<rsf_dlss_native12> registration_owner;
#if RSF_HAVE_STREAMLINE
    PFun_slShutdown* shutdown = nullptr;
    PFun_slGetNewFrameToken* token = nullptr;
    PFun_slSetConstants* constants = nullptr;
    PFun_slSetTagForFrame* tags = nullptr;
    PFun_slEvaluateFeature* evaluate = nullptr;
    PFun_slFreeResources* free_resources = nullptr;
    PFun_slDLSSGetOptimalSettings* optimal = nullptr;
    PFun_slDLSSSetOptions* options = nullptr;
    HRESULT (*present)(IDXGISwapChain*, UINT, UINT, bool&){};
    HRESULT (*after_present)(UINT){};
#endif
};
struct rsf_dlss_native12_registration {
    std::shared_ptr<rsf_dlss_native12> owner;
};
static rsf_backend_result create_context(void* device, const rsf_dlss_setup* setup, rsf_streamline_host* shared,
    rsf_dlss_native12** out, HMODULE engine_module = nullptr) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!device || !setup || setup->struct_size < sizeof(*setup) || !setup->interposer_path_utf8 ||
        !setup->plugin_directory_utf8 || !setup->engine_version_utf8 ||
        (!setup->application_id && !setup->project_id_utf8))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!rsf_dlss_preset_valid(setup->preset)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_DLSS_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if RSF_HAVE_STREAMLINE
    // Shared SR borrows the presentation host; standalone SR owns one registration.
    if (!shared && !engine_module && GetModuleHandleW(L"sl.interposer.dll")) return RSF_BACKEND_ERROR_NOT_READY;
    std::unique_ptr<rsf_dlss_native12, decltype(&rsf_dlss_native12_destroy)> context(new rsf_dlss_native12, rsf_dlss_native12_destroy);
    std::wstring path, directory;
    if (!rsf::widen_utf8(setup->interposer_path_utf8, path) || !rsf::widen_utf8(setup->plugin_directory_utf8, directory))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    context->shared_host = shared;
    context->preset = setup->preset;
    if (shared) {
        if (setup->require_signature && !rsf_dlss_verify_runtime_signature(path.c_str()))
            return RSF_BACKEND_ERROR_LOAD_FAILED;
        rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
        if (rsf_streamline_host_graphics(shared, &graphics) != RSF_BACKEND_OK || graphics.native_device != device)
            return RSF_BACKEND_ERROR_WRONG_API;
        context->module = static_cast<HMODULE>(rsf_streamline_host_module(shared));
    } else if (engine_module) {
        wchar_t existing_path[32768]{};
        const auto length = GetModuleFileNameW(engine_module, existing_path, 32768);
        if (!length || length >= 32768 || (setup->require_signature && !rsf_dlss_verify_runtime_signature(existing_path)))
            return RSF_BACKEND_ERROR_LOAD_FAILED;
        HMODULE retained{};
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                reinterpret_cast<const wchar_t*>(engine_module), &retained) || retained != engine_module)
            return RSF_BACKEND_ERROR_LOAD_FAILED;
        context->module = retained;
        context->borrowed_engine = true;
    } else {
        context->module = rsf::load_interposer(path.c_str(), setup->require_signature != 0, setup->log, setup->log_user);
    }
    if (!context->module) return RSF_BACKEND_ERROR_LOAD_FAILED;
    HMODULE module = context->module;
    auto* init = rsf::entry<PFun_slInit*>(module, "slInit");
    auto* set_device = rsf::entry<PFun_slSetD3DDevice*>(module, "slSetD3DDevice");
    auto* support = rsf::entry<PFun_slIsFeatureSupported*>(module, "slIsFeatureSupported");
    auto* feature = rsf::entry<PFun_slGetFeatureFunction*>(module, "slGetFeatureFunction");
    context->shutdown = rsf::entry<PFun_slShutdown*>(module, "slShutdown");
    context->token = rsf::entry<PFun_slGetNewFrameToken*>(module, "slGetNewFrameToken");
    context->constants = rsf::entry<PFun_slSetConstants*>(module, "slSetConstants");
    context->tags = rsf::entry<PFun_slSetTagForFrame*>(module, "slSetTagForFrame");
    context->evaluate = rsf::entry<PFun_slEvaluateFeature*>(module, "slEvaluateFeature");
    context->free_resources = rsf::entry<PFun_slFreeResources*>(module, "slFreeResources");
    if (!init || !set_device || !support || !feature || !context->shutdown || !context->token ||
        !context->constants || !context->tags || !context->evaluate || !context->free_resources)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    const wchar_t* paths[] = {directory.c_str()};
    sl::Feature features[] = {sl::kFeatureDLSS};
    sl::Preferences preferences{};
    preferences.renderAPI = sl::RenderAPI::eD3D12;
    preferences.flags = sl::PreferenceFlags::eUseManualHooking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.featuresToLoad = features; preferences.numFeaturesToLoad = 1;
    preferences.pathsToPlugins = paths; preferences.numPathsToPlugins = 1;
    preferences.engine = setup->engine == RSF_DLSS_ENGINE_UNITY ? sl::EngineType::eUnity : sl::EngineType::eCustom;
    preferences.applicationId = setup->application_id;
    preferences.engineVersion = setup->engine_version_utf8; preferences.projectId = setup->project_id_utf8;
    if (!shared && !engine_module) {
        if (init(preferences, sl::kSDKVersion) != sl::Result::eOk) return RSF_BACKEND_ERROR_INIT_FAILED;
        context->initialized = true;
        if (set_device(device) != sl::Result::eOk) return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    context->device = static_cast<ID3D12Device*>(device);
    LUID luid = context->device->GetAdapterLuid();
    sl::AdapterInfo adapter{}; adapter.deviceLUID = reinterpret_cast<uint8_t*>(&luid); adapter.deviceLUIDSizeInBytes = sizeof(luid);
    const auto supported = support(sl::kFeatureDLSS, adapter);
    if (supported != sl::Result::eOk) {
        if (setup->log) { char message[128]; std::snprintf(message, sizeof(message), "DLSS native D3D12 support query returned Streamline result %u", static_cast<uint32_t>(supported)); setup->log(setup->log_user, message); }
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    void* optimal = nullptr; void* options = nullptr;
    if (feature(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", optimal) != sl::Result::eOk ||
        feature(sl::kFeatureDLSS, "slDLSSSetOptions", options) != sl::Result::eOk || !optimal || !options)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    context->optimal = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal);
    context->options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    if (!shared && !engine_module) {
        void* before = nullptr; void* after = nullptr;
        if (feature(sl::kFeatureCommon, "slHookPresent", before) != sl::Result::eOk ||
            feature(sl::kFeatureCommon, "slHookAfterPresent", after) != sl::Result::eOk || !before || !after)
            return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
        context->present = reinterpret_cast<decltype(context->present)>(before);
        context->after_present = reinterpret_cast<decltype(context->after_present)>(after);
    }
    if (setup->log) setup->log(setup->log_user, "DLSS SR registered on the caller's native D3D12 device.");
    *out = context.release(); return RSF_BACKEND_OK;
#else
    (void)shared;
    (void)engine_module;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
rsf_backend_result rsf_dlss_native12_registration_create(void* device, const rsf_dlss_setup* setup,
    rsf_dlss_native12_registration** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    auto registration = std::make_unique<rsf_dlss_native12_registration>();
    rsf_dlss_native12* owner{};
    const auto result = create_context(device, setup, nullptr, &owner);
    if (result != RSF_BACKEND_OK) return result;
    registration->owner = std::shared_ptr<rsf_dlss_native12>(owner, rsf_dlss_native12_destroy);
    *out = registration.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
rsf_backend_result rsf_dlss_native12_registration_borrow(void* device, void* module,
    const rsf_dlss_setup* setup, rsf_dlss_native12_registration** out) try
{
    if (!out || !module) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    auto registration = std::make_unique<rsf_dlss_native12_registration>();
    rsf_dlss_native12* owner{};
    const auto result = create_context(device, setup, nullptr, &owner, static_cast<HMODULE>(module));
    if (result != RSF_BACKEND_OK) return result;
    registration->owner = std::shared_ptr<rsf_dlss_native12>(owner, rsf_dlss_native12_destroy);
    *out = registration.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
rsf_backend_result rsf_dlss_native12_create_registered(void* device, const rsf_dlss_setup* setup,
    rsf_dlss_native12_registration* registration, rsf_dlss_native12** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || !registration || !registration->owner ||
        device != registration->owner->device.Get() || !rsf_dlss_preset_valid(setup->preset))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_DLSS_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    auto context = std::make_unique<rsf_dlss_native12>(*registration->owner);
    context->initialized = false;
    context->registration_owner = registration->owner;
    context->preset = setup->preset;
    *out = context.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
rsf_backend_result rsf_dlss_native12_registration_present(rsf_dlss_native12_registration* registration,
    void* swapchain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    return registration && registration->owner ? rsf_dlss_native12_present(registration->owner.get(), swapchain,
        interval, flags, begin) : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
void rsf_dlss_native12_registration_destroy(rsf_dlss_native12_registration* registration)
{
    delete registration;
}
rsf_backend_result rsf_dlss_native12_present(rsf_dlss_native12* context, void* swapchain,
    uint32_t sync_interval, uint32_t flags, uint32_t begin)
{
    if (!context || !swapchain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (context->shared_host || context->borrowed_engine) return RSF_BACKEND_OK;
#if RSF_HAVE_STREAMLINE
    bool skip = false;
    const auto result = begin ? context->present(static_cast<IDXGISwapChain*>(swapchain), sync_interval, flags, skip) :
        context->after_present(flags);
    return SUCCEEDED(result) && !skip ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
#else
    (void)sync_interval; (void)flags; (void)begin;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
rsf_backend_result rsf_dlss_native12_create(void* device, const rsf_dlss_setup* setup, rsf_dlss_native12** out) {
    return create_context(device, setup, nullptr, out);
}
rsf_backend_result rsf_dlss_native12_create_shared(void* device, const rsf_dlss_setup* setup, rsf_streamline_host* host, rsf_dlss_native12** out) {
    return create_context(device, setup, host, out);
}
rsf_backend_result rsf_dlss_native12_plan(rsf_dlss_native12* context, uint32_t width, uint32_t height,
    rsf_quality quality, uint32_t* render_width, uint32_t* render_height)
{
    if (!context || !width || !height || !render_width || !render_height || quality > RSF_QUALITY_ULTRA_PERFORMANCE)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    sl::DLSSOptions options{}; options.mode = rsf::dlss_mode(quality); options.outputWidth = width; options.outputHeight = height;
    sl::DLSSOptimalSettings settings{};
    if (context->optimal(options, settings) != sl::Result::eOk || !settings.optimalRenderWidth || !settings.optimalRenderHeight)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *render_width = settings.optimalRenderWidth; *render_height = settings.optimalRenderHeight;
    context->quality = quality;
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
static rsf_backend_result evaluate_frame(rsf_dlss_native12* context, void* list, const rsf_sr_frame* frame, void* engine_token)
{
    if (!context || !list) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    // The same checks FSR and XeSS apply, so the three backends accept the same frames.
    auto valid = rsf::validate_frame(frame);
    if (valid == RSF_BACKEND_OK) valid = rsf::validate_d3d12_resources(*frame, context->device.Get());
    if (valid != RSF_BACKEND_OK) return valid;
    const auto& record = *frame->record;
    const auto& camera = record.camera;
    uint32_t index = static_cast<uint32_t>(record.frame_id);
    sl::FrameToken* token = nullptr;
    if (context->borrowed_engine) token = static_cast<sl::FrameToken*>(engine_token);
    else if (context->shared_host) token = static_cast<sl::FrameToken*>(rsf_streamline_host_token(context->shared_host, record.frame_id));
    else if (context->token(token, &index) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    // SR consumes pre-tonemap inputs and has independent history/reset constants.
    // The shared host reserves viewport zero for completed-frame generation.
    sl::ViewportHandle viewport(context->shared_host || context->borrowed_engine ? 1u : 0u);
    sl::Constants constants{};
    constants.cameraViewToClip = rsf::sl_matrix(camera.view_to_clip); constants.clipToCameraView = rsf::sl_matrix(camera.clip_to_view);
    constants.clipToPrevClip = rsf::sl_matrix(camera.clip_to_previous_clip);
    DirectX::XMFLOAT4X4 copied{}, inverted{};
    std::memcpy(&copied, camera.clip_to_previous_clip, sizeof(copied));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverted, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&copied)));
    const float det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    constants.prevClipToClip = rsf::sl_matrix(&inverted.m[0][0]);
    constants.jitterOffset = {frame->jitter_x, frame->jitter_y};
    constants.mvecScale = {1.0f / record.render_width, 1.0f / record.render_height};
    constants.cameraPinholeOffset = {0, 0};
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = rsf::sl_camera_forward(camera,-1.0f);
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane;
    constants.cameraFOV = camera.vertical_fov_radians;
    constants.cameraAspectRatio = static_cast<float>(record.output_width) / record.output_height;
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue;
    constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max();
    constants.reset = frame->reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    if (context->constants(constants, *token, viewport) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    sl::DLSSOptions options{}; options.mode = rsf::dlss_mode(context->quality);
    const auto preset = static_cast<sl::DLSSPreset>(context->preset);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = preset;
    options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    options.outputWidth = record.output_width; options.outputHeight = record.output_height;
    options.colorBuffersHDR = context->sdr ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    options.useAutoExposure = context->sdr || frame->exposure.resource ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    if (context->options(viewport, options) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    sl::Resource input(sl::ResourceType::eTex2d, frame->color.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Resource depth(sl::ResourceType::eTex2d, frame->depth.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Resource motion(sl::ResourceType::eTex2d, frame->motion.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Resource output(sl::ResourceType::eTex2d, frame->output.resource, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    sl::Resource exposure(sl::ResourceType::eTex2d, frame->exposure.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Extent render{0, 0, record.render_width, record.render_height};
    sl::Extent presented{0, 0, record.output_width, record.output_height};
    sl::ResourceTag tags[] = {
        {&input, sl::kBufferTypeScalingInputColor, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&motion, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&output, sl::kBufferTypeScalingOutputColor, sl::ResourceLifecycle::eOnlyValidNow, &presented},
        {&exposure, sl::kBufferTypeExposure, sl::ResourceLifecycle::eOnlyValidNow, nullptr}};
    if (context->tags(*token, viewport, tags, frame->exposure.resource ? 5u : 4u, list) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const sl::BaseStructure* inputs[] = {&viewport};
    return context->evaluate(sl::kFeatureDLSS, *token, inputs, 1, list) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
#else
    (void)frame;
    (void)engine_token;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
rsf_backend_result rsf_dlss_native12_evaluate(rsf_dlss_native12* context, void* list, const rsf_sr_frame* frame)
{
    return evaluate_frame(context, list, frame, nullptr);
}
rsf_backend_result rsf_dlss_native12_evaluate_borrowed(rsf_dlss_native12* context, void* list,
    const rsf_sr_frame* frame, void* native_token)
{
    if (!context || !context->borrowed_engine || !native_token) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return evaluate_frame(context, list, frame, native_token);
}
void rsf_dlss_native12_destroy(rsf_dlss_native12* context)
{
    if (!context) return;
#if RSF_HAVE_STREAMLINE
    if (context->free_resources && context->module) {
        const sl::ViewportHandle viewport(context->shared_host || context->borrowed_engine ? 1u : 0u);
        (void)context->free_resources(sl::kFeatureDLSS, viewport);
    }
    if (context->initialized && context->shutdown) context->shutdown();
#endif
    if (context->module && !context->shared_host && !context->registration_owner) FreeLibrary(context->module);
    delete context;
}
void rsf_dlss_native12_set_sdr(rsf_dlss_native12* context, uint32_t sdr)
{
    if (context) context->sdr = sdr != 0;
}
