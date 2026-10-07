// SPDX-License-Identifier: GPL-3.0-only
// DX12 SR uses a private viewport when borrowing the presentation host, keeping pre-tonemap
// reconstruction history separate from completed-frame generation on viewport zero.
#include <rescaleframe/dlss_native12.h>
#include <windows.h>
#include <d3d12.h>
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
// The existing DLSS translation unit owns Streamline's non-inline signature verifier.
namespace sl::security { bool verifyEmbeddedSignature(const wchar_t* path); }
#endif

/** Context lifetime distinguishes a borrowed host/module from an owned standalone registration. */
struct rsf_dlss_native12 {
    HMODULE module = nullptr;
    bool initialized = false;
    bool owned_module = false;
    rsf_streamline_host* shared_host = nullptr;
    rsf_quality quality = RSF_QUALITY_QUALITY;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
#if RSF_HAVE_STREAMLINE
    PFun_slShutdown* shutdown = nullptr;
    PFun_slGetNewFrameToken* token = nullptr;
    PFun_slSetConstants* constants = nullptr;
    PFun_slSetTagForFrame* tags = nullptr;
    PFun_slEvaluateFeature* evaluate = nullptr;
    PFun_slFreeResources* free_resources = nullptr;
    PFun_slDLSSGetOptimalSettings* optimal = nullptr;
    PFun_slDLSSSetOptions* options = nullptr;
#endif
};
#if RSF_HAVE_STREAMLINE
namespace {
/* Bind an official SDK function type; null remains a reported missing-export failure. */
template<typename T> bool entry(HMODULE module, const char* name, T*& out)
{
    out = reinterpret_cast<T*>(reinterpret_cast<void*>(GetProcAddress(module, name)));
    return out != nullptr;
}
/* Public entry points validate quality before reaching this native/DLSS mode mapping. */
sl::DLSSMode mode(rsf_quality quality)
{
    switch (quality) {
    case RSF_QUALITY_NATIVE: return sl::DLSSMode::eDLAA;
    case RSF_QUALITY_QUALITY: return sl::DLSSMode::eMaxQuality;
    case RSF_QUALITY_BALANCED: return sl::DLSSMode::eBalanced;
    case RSF_QUALITY_PERFORMANCE: return sl::DLSSMode::eMaxPerformance;
    default: return sl::DLSSMode::eUltraPerformance;
    }
}
/* Both contracts use row-major matrices, so no transpose or axis conversion is performed. */
sl::float4x4 matrix(const float* values)
{
    sl::float4x4 out{};
    for (size_t i = 0; i < 4; ++i) out.row[i] = {values[i*4], values[i*4+1], values[i*4+2], values[i*4+3]};
    return out;
}
}
#endif
/** Build standalone SR or borrow a matching host. The unique_ptr owns every partial state until
 * publication, and exceptions return INIT_FAILED. Shared creation neither initializes nor shuts
 * down the host SDK; standalone creation rejects any independently loaded interposer.
 */
static rsf_backend_result create_context(void* device, const rsf_dlss_setup* setup, rsf_streamline_host* shared, rsf_dlss_native12** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!device || !setup || setup->struct_size < sizeof(*setup) || !setup->interposer_path_utf8 ||
        !setup->plugin_directory_utf8 || !setup->engine_version_utf8 || !setup->project_id_utf8)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_DLSS_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if RSF_HAVE_STREAMLINE
    // Shared SR borrows the presentation host; standalone SR owns one registration.
    if (!shared && GetModuleHandleW(L"sl.interposer.dll")) return RSF_BACKEND_ERROR_NOT_READY;
    std::unique_ptr<rsf_dlss_native12, decltype(&rsf_dlss_native12_destroy)> context(new rsf_dlss_native12, rsf_dlss_native12_destroy);
    auto widen = [](const char* text) {
        int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, -1, nullptr, 0);
        std::wstring result(length > 0 ? static_cast<size_t>(length) : 0, L'\0');
        if (length > 0) MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, -1, result.data(), length);
        return result;
    };
    auto path = widen(setup->interposer_path_utf8), directory = widen(setup->plugin_directory_utf8);
    if (path.empty() || directory.empty()) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->require_signature && !sl::security::verifyEmbeddedSignature(path.c_str()))
        return RSF_BACKEND_ERROR_LOAD_FAILED;
    context->shared_host = shared;
    if (shared) {
        rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
        if (rsf_streamline_host_graphics(shared, &graphics) != RSF_BACKEND_OK || graphics.native_device != device)
            return RSF_BACKEND_ERROR_WRONG_API;
        context->module = static_cast<HMODULE>(rsf_streamline_host_module(shared));
    } else {
        context->module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        context->owned_module = context->module != nullptr;
    }
    if (!context->module) return RSF_BACKEND_ERROR_LOAD_FAILED;
    PFun_slInit* init = nullptr;
    PFun_slSetD3DDevice* set_device = nullptr;
    PFun_slIsFeatureSupported* support = nullptr;
    PFun_slGetFeatureFunction* feature = nullptr;
    if (!entry(context->module, "slInit", init) || !entry(context->module, "slSetD3DDevice", set_device) ||
        !entry(context->module, "slIsFeatureSupported", support) || !entry(context->module, "slGetFeatureFunction", feature) ||
        !entry(context->module, "slShutdown", context->shutdown) || !entry(context->module, "slGetNewFrameToken", context->token) ||
        !entry(context->module, "slSetConstants", context->constants) || !entry(context->module, "slSetTagForFrame", context->tags) ||
        !entry(context->module, "slEvaluateFeature", context->evaluate) || !entry(context->module, "slFreeResources", context->free_resources))
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    const wchar_t* paths[] = {directory.c_str()};
    sl::Feature features[] = {sl::kFeatureDLSS};
    sl::Preferences preferences{};
    preferences.renderAPI = sl::RenderAPI::eD3D12;
    preferences.flags = sl::PreferenceFlags::eUseManualHooking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.featuresToLoad = features; preferences.numFeaturesToLoad = 1;
    preferences.pathsToPlugins = paths; preferences.numPathsToPlugins = 1;
    preferences.engine = setup->engine == RSF_DLSS_ENGINE_UNITY ? sl::EngineType::eUnity : sl::EngineType::eCustom;
    preferences.engineVersion = setup->engine_version_utf8; preferences.projectId = setup->project_id_utf8;
    if (!shared) {
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
    if (setup->log) setup->log(setup->log_user, "DLSS SR registered on the caller's native D3D12 device.");
    *out = context.release(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
rsf_backend_result rsf_dlss_native12_create(void* device, const rsf_dlss_setup* setup, rsf_dlss_native12** out) {
    return create_context(device, setup, nullptr, out);
}
rsf_backend_result rsf_dlss_native12_create_shared(void* device, const rsf_dlss_setup* setup, rsf_streamline_host* host, rsf_dlss_native12** out) {
    return create_context(device, setup, host, out);
}
/** Ask the runtime for valid optimal input dimensions and retain quality for evaluate. */
rsf_backend_result rsf_dlss_native12_plan(rsf_dlss_native12* context, uint32_t width, uint32_t height,
    rsf_quality quality, uint32_t* render_width, uint32_t* render_height)
{
    if (!context || !width || !height || !render_width || !render_height || quality > RSF_QUALITY_ULTRA_PERFORMANCE)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    sl::DLSSOptions options{}; options.mode = mode(quality); options.outputWidth = width; options.outputHeight = height;
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
/** Record SR on a caller-owned list with decoded, complete, non-jittered motion in pixel units.
 * A shared call requires the host's still-live source token. This path uses HDR/auto exposure,
 * fixed shader/UAV states, and the previously planned quality; the caller submits and retires work.
 */
rsf_backend_result rsf_dlss_native12_evaluate(rsf_dlss_native12* context, void* list, const rsf_sr_frame* frame)
{
    if (!context || !list || !frame || frame->struct_size < sizeof(*frame) || !frame->record ||
        !frame->color.resource || !frame->depth.resource || !frame->motion.resource || !frame->output.resource)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    const auto& record = *frame->record;
    const auto& camera = record.camera;
    uint32_t index = static_cast<uint32_t>(record.frame_id);
    sl::FrameToken* token = nullptr;
    if (context->shared_host) token = static_cast<sl::FrameToken*>(rsf_streamline_host_token(context->shared_host, record.frame_id));
    else if (context->token(token, &index) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    // SR consumes pre-tonemap inputs and has independent history/reset constants.
    // The shared host reserves viewport zero for completed-frame generation.
    sl::ViewportHandle viewport(context->shared_host ? 1u : 0u);
    sl::Constants constants{};
    constants.cameraViewToClip = matrix(camera.view_to_clip); constants.clipToCameraView = matrix(camera.clip_to_view);
    constants.clipToPrevClip = matrix(camera.clip_to_previous_clip);
    DirectX::XMFLOAT4X4 copied{}, inverted{};
    std::memcpy(&copied, camera.clip_to_previous_clip, sizeof(copied));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverted, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&copied)));
    const float det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    constants.prevClipToClip = matrix(&inverted.m[0][0]);
    constants.jitterOffset = {frame->jitter_x, frame->jitter_y};
    // This native path expects already resolved pixel motion rather than frame.motion_scale.
    constants.mvecScale = {1.0f / record.render_width, 1.0f / record.render_height};
    constants.cameraPinholeOffset = {0, 0};
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {-camera.view_to_world[8], -camera.view_to_world[9], -camera.view_to_world[10]};
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane;
    constants.cameraFOV = camera.vertical_fov_radians;
    constants.cameraAspectRatio = static_cast<float>(record.output_width) / record.output_height;
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue;
    constants.motionVectorsJittered = sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max();
    constants.reset = frame->reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    if (context->constants(constants, *token, viewport) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    sl::DLSSOptions options{}; options.mode = mode(context->quality);
    options.outputWidth = record.output_width; options.outputHeight = record.output_height;
    options.colorBuffersHDR = sl::Boolean::eTrue; options.useAutoExposure = sl::Boolean::eTrue;
    if (context->options(viewport, options) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    sl::Resource input(sl::ResourceType::eTex2d, frame->color.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Resource depth(sl::ResourceType::eTex2d, frame->depth.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Resource motion(sl::ResourceType::eTex2d, frame->motion.resource, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    sl::Resource output(sl::ResourceType::eTex2d, frame->output.resource, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    sl::Extent render{0, 0, record.render_width, record.render_height};
    sl::Extent presented{0, 0, record.output_width, record.output_height};
    sl::ResourceTag tags[] = {
        {&input, sl::kBufferTypeScalingInputColor, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&motion, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&output, sl::kBufferTypeScalingOutputColor, sl::ResourceLifecycle::eOnlyValidNow, &presented}};
    if (context->tags(*token, viewport, tags, 4, list) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const sl::BaseStructure* inputs[] = {&viewport};
    return context->evaluate(sl::kFeatureDLSS, *token, inputs, 1, list) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
/** Dispose after GPU retirement; only the standalone context owns shutdown and module unloading. */
void rsf_dlss_native12_destroy(rsf_dlss_native12* context)
{
    if (!context) return;
#if RSF_HAVE_STREAMLINE
    if (context->initialized && context->shutdown) context->shutdown();
#endif
    if (context->module && context->owned_module) FreeLibrary(context->module);
    delete context;
}
