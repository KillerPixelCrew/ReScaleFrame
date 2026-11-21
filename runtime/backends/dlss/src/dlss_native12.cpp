// SPDX-License-Identifier: MIT
#include <rescaleframe/dlss_native12.h>

#if RSF_HAVE_STREAMLINE
#include "streamline_frame.h"
#include "streamline_load.h"
#include <sl_consts.h>
#include <DirectXMath.h>
#include <dxgi.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <memory>

namespace {
// Typed SDK entry points and preferences. The Rust registration leases the module
// and keeps the borrowed plugin directory alive.
struct Native {
    const wchar_t* plugin_paths[1]{};
    PFun_slInit* init = nullptr;
    PFun_slSetD3DDevice* set_device = nullptr;
    PFun_slIsFeatureSupported* support = nullptr;
    PFun_slGetFeatureFunction* feature = nullptr;
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
};

void* native_load(const wchar_t* path, const rsf_dlss_setup* setup) noexcept try
{
    return rsf::load_interposer(path, setup->require_signature != 0, setup->log, setup->log_user);
} catch (...) { return nullptr; }

bool native_verify(const wchar_t* path) noexcept try
{
    return rsf_dlss_verify_runtime_signature(path);
} catch (...) { return false; }

rsf_backend_result native_shared_module(void* device, void* shared_host, void** out) noexcept try
{
    auto* host = static_cast<rsf_streamline_host*>(shared_host);
    rsf_streamline_graphics graphics{};
    graphics.struct_size = sizeof(graphics);
    if (rsf_streamline_host_graphics(host, &graphics) != RSF_BACKEND_OK || graphics.native_device != device)
        return RSF_BACKEND_ERROR_WRONG_API;
    *out = rsf_streamline_host_module(host);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_resolve(void* handle, const wchar_t* directory, void** out) noexcept try
{
    const auto module = static_cast<HMODULE>(handle);
    auto native = std::make_unique<Native>();
    native->plugin_paths[0] = directory;
    native->init = rsf::entry<PFun_slInit*>(module, "slInit");
    native->set_device = rsf::entry<PFun_slSetD3DDevice*>(module, "slSetD3DDevice");
    native->support = rsf::entry<PFun_slIsFeatureSupported*>(module, "slIsFeatureSupported");
    native->feature = rsf::entry<PFun_slGetFeatureFunction*>(module, "slGetFeatureFunction");
    native->shutdown = rsf::entry<PFun_slShutdown*>(module, "slShutdown");
    native->token = rsf::entry<PFun_slGetNewFrameToken*>(module, "slGetNewFrameToken");
    native->constants = rsf::entry<PFun_slSetConstants*>(module, "slSetConstants");
    native->tags = rsf::entry<PFun_slSetTagForFrame*>(module, "slSetTagForFrame");
    native->evaluate = rsf::entry<PFun_slEvaluateFeature*>(module, "slEvaluateFeature");
    native->free_resources = rsf::entry<PFun_slFreeResources*>(module, "slFreeResources");
    if (!native->init || !native->set_device || !native->support || !native->feature ||
        !native->shutdown || !native->token || !native->constants || !native->tags ||
        !native->evaluate || !native->free_resources)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    *out = native.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_init(void* handle, const rsf_dlss_setup* setup) noexcept try
{
    auto& native = *static_cast<Native*>(handle);
    static constexpr sl::Feature features[] = {sl::kFeatureDLSS};
    sl::Preferences preferences{};
    preferences.renderAPI = sl::RenderAPI::eD3D12;
    preferences.flags = sl::PreferenceFlags::eUseManualHooking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.featuresToLoad = features;
    preferences.numFeaturesToLoad = 1;
    preferences.pathsToPlugins = native.plugin_paths;
    preferences.numPathsToPlugins = 1;
    preferences.engine = setup->engine == RSF_DLSS_ENGINE_UNITY ? sl::EngineType::eUnity : sl::EngineType::eCustom;
    preferences.applicationId = setup->application_id;
    preferences.engineVersion = setup->engine_version_utf8;
    preferences.projectId = setup->project_id_utf8;
    return native.init(preferences, sl::kSDKVersion) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_set_device(void* handle, void* device) noexcept try
{
    return static_cast<Native*>(handle)->set_device(device) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_configure(void* handle, void* device, const rsf_dlss_setup* setup,
    uint32_t owns_present) noexcept try
{
    auto& native = *static_cast<Native*>(handle);
    LUID luid = static_cast<ID3D12Device*>(device)->GetAdapterLuid();
    sl::AdapterInfo adapter{};
    adapter.deviceLUID = reinterpret_cast<uint8_t*>(&luid);
    adapter.deviceLUIDSizeInBytes = sizeof(luid);
    const auto supported = native.support(sl::kFeatureDLSS, adapter);
    if (supported != sl::Result::eOk) {
        if (setup->log) {
            char message[128];
            std::snprintf(message, sizeof(message), "DLSS native D3D12 support query returned Streamline result %u",
                static_cast<uint32_t>(supported));
            setup->log(setup->log_user, message);
        }
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    void* optimal = nullptr;
    void* options = nullptr;
    if (native.feature(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", optimal) != sl::Result::eOk ||
        native.feature(sl::kFeatureDLSS, "slDLSSSetOptions", options) != sl::Result::eOk || !optimal || !options)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    native.optimal = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal);
    native.options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    if (owns_present) {
        void* before = nullptr;
        void* after = nullptr;
        if (native.feature(sl::kFeatureCommon, "slHookPresent", before) != sl::Result::eOk ||
            native.feature(sl::kFeatureCommon, "slHookAfterPresent", after) != sl::Result::eOk || !before || !after)
            return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
        native.present = reinterpret_cast<decltype(native.present)>(before);
        native.after_present = reinterpret_cast<decltype(native.after_present)>(after);
    }
    if (setup->log) setup->log(setup->log_user, "DLSS SR registered on the caller's native D3D12 device.");
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void native_shutdown(void* handle) noexcept try
{
    (void)static_cast<Native*>(handle)->shutdown();
} catch (...) {}

void native_close(void* handle) noexcept { delete static_cast<Native*>(handle); }

rsf_backend_result native_present(void* handle, void* swapchain, uint32_t interval, uint32_t flags,
    uint32_t begin) noexcept try
{
    const auto& native = *static_cast<Native*>(handle);
    bool skip = false;
    const auto result = begin ? native.present(static_cast<IDXGISwapChain*>(swapchain), interval, flags, skip) :
        native.after_present(flags);
    return SUCCEEDED(result) && !skip ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_plan(void* handle, uint32_t width, uint32_t height, uint32_t quality,
    uint32_t* render_width, uint32_t* render_height) noexcept try
{
    sl::DLSSOptions options{};
    options.mode = rsf::dlss_mode(quality);
    options.outputWidth = width;
    options.outputHeight = height;
    sl::DLSSOptimalSettings settings{};
    if (static_cast<Native*>(handle)->optimal(options, settings) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *render_width = settings.optimalRenderWidth;
    *render_height = settings.optimalRenderHeight;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_validate(void* device, const rsf_sr_frame* frame) noexcept try
{
    const auto result = rsf::validate_frame(frame);
    return result == RSF_BACKEND_OK ? rsf::validate_d3d12_resources(*frame, device) : result;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_token(void* handle, uint32_t index, void** out) noexcept try
{
    sl::FrameToken* token = nullptr;
    if (static_cast<Native*>(handle)->token(token, &index) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *out = token;
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void* native_shared_token(void* host, uint64_t frame_id) noexcept try
{
    return rsf_streamline_host_token(static_cast<rsf_streamline_host*>(host), frame_id);
} catch (...) { return nullptr; }

rsf_backend_result native_constants(void* handle, void* token_ptr, uint32_t viewport_id,
    const rsf_sr_frame* frame) noexcept try
{
    const auto& native = *static_cast<Native*>(handle);
    const auto* token = static_cast<sl::FrameToken*>(token_ptr);
    const sl::ViewportHandle viewport(viewport_id);
    const auto& record = *frame->record;
    const auto& camera = record.camera;
    sl::Constants constants{};
    constants.cameraViewToClip = rsf::sl_matrix(camera.view_to_clip);
    constants.clipToCameraView = rsf::sl_matrix(camera.clip_to_view);
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
    constants.cameraFwd = rsf::sl_camera_forward(camera, -1.0f);
    constants.cameraNear = camera.near_plane;
    constants.cameraFar = camera.far_plane;
    constants.cameraFOV = camera.vertical_fov_radians;
    constants.cameraAspectRatio = static_cast<float>(record.output_width) / record.output_height;
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue;
    constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max();
    constants.reset = frame->reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    return native.constants(constants, *token, viewport) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_options(void* handle, uint32_t viewport, const rsf_sr_frame* frame,
    uint32_t quality, uint32_t preset_value, uint32_t sdr) noexcept try
{
    const auto& native = *static_cast<Native*>(handle);
    const auto& record = *frame->record;
    sl::DLSSOptions options{};
    options.mode = rsf::dlss_mode(quality);
    const auto preset = static_cast<sl::DLSSPreset>(preset_value);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = preset;
    options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    options.outputWidth = record.output_width;
    options.outputHeight = record.output_height;
    options.colorBuffersHDR = sdr ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    options.useAutoExposure = sdr || frame->exposure.resource ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    return native.options(sl::ViewportHandle(viewport), options) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_dispatch(void* handle, void* token_ptr, uint32_t viewport_id, void* list,
    const rsf_sr_frame* frame) noexcept try
{
    const auto& native = *static_cast<Native*>(handle);
    const auto* token = static_cast<sl::FrameToken*>(token_ptr);
    const sl::ViewportHandle viewport(viewport_id);
    const auto& record = *frame->record;
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
    if (native.tags(*token, viewport, tags, frame->exposure.resource ? 5u : 4u, list) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const sl::BaseStructure* inputs[] = {&viewport};
    return native.evaluate(sl::kFeatureDLSS, *token, inputs, 1, list) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void native_free_viewport(void* handle, uint32_t viewport) noexcept try
{
    (void)static_cast<Native*>(handle)->free_resources(sl::kFeatureDLSS, sl::ViewportHandle(viewport));
} catch (...) {}

struct Platform {
    decltype(&native_load) load;
    decltype(&native_verify) verify;
    decltype(&native_shared_module) shared_module;
    decltype(&native_resolve) resolve;
    decltype(&native_init) init;
    decltype(&native_set_device) set_device;
    decltype(&native_configure) configure;
    decltype(&native_shutdown) shutdown;
    decltype(&native_close) close;
    decltype(&native_present) present;
    decltype(&native_plan) plan;
    decltype(&native_validate) validate;
    decltype(&native_token) token;
    decltype(&native_shared_token) shared_token;
    decltype(&native_constants) constants;
    decltype(&native_options) options;
    decltype(&native_dispatch) dispatch;
    decltype(&native_free_viewport) free_viewport;
};
static_assert(sizeof(Platform) == 18 * sizeof(void*));
static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_BACKEND_ERROR_ABI_MISMATCH == -2 && RSF_BACKEND_ERROR_LOAD_FAILED == -4 && RSF_BACKEND_ERROR_INIT_FAILED == -6 &&
    RSF_BACKEND_ERROR_NOT_READY == -8 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9 &&
    RSF_QUALITY_QUALITY == 1);

const Platform platform{
    native_load, native_verify, native_shared_module, native_resolve,
    native_init, native_set_device, native_configure, native_shutdown, native_close, native_present,
    native_plan, native_validate, native_token, native_shared_token, native_constants, native_options,
    native_dispatch, native_free_viewport,
};
}

extern "C" {
rsf_backend_result rsf_core_dlss_native12_create(void*, const rsf_dlss_setup*, void*,
    rsf_dlss_native12**, const Platform*);
rsf_backend_result rsf_core_dlss_native12_register(void*, const rsf_dlss_setup*, void*,
    rsf_dlss_native12_registration**, const Platform*);
rsf_backend_result rsf_core_dlss_native12_create_registered(void*, const rsf_dlss_setup*,
    const rsf_dlss_native12_registration*, rsf_dlss_native12**);
rsf_backend_result rsf_core_dlss_native12_registration_present(const rsf_dlss_native12_registration*,
    void*, uint32_t, uint32_t, uint32_t);
void rsf_core_dlss_native12_registration_destroy(rsf_dlss_native12_registration*);
rsf_backend_result rsf_core_dlss_native12_present(const rsf_dlss_native12*, void*, uint32_t, uint32_t, uint32_t);
rsf_backend_result rsf_core_dlss_native12_plan(const rsf_dlss_native12*, uint32_t, uint32_t, rsf_quality,
    uint32_t*, uint32_t*);
rsf_backend_result rsf_core_dlss_native12_evaluate(const rsf_dlss_native12*, void*, const rsf_sr_frame*, void*);
void rsf_core_dlss_native12_destroy(rsf_dlss_native12*);
void rsf_core_dlss_native12_set_sdr(const rsf_dlss_native12*, uint32_t);
}
#endif

namespace {
rsf_backend_result validate_setup(void* device, const rsf_dlss_setup* setup)
{
    if (!device || !setup || setup->struct_size < sizeof(*setup) || !setup->interposer_path_utf8 ||
        !setup->plugin_directory_utf8 || !setup->engine_version_utf8 ||
        (!setup->application_id && !setup->project_id_utf8) || !rsf_dlss_preset_valid(setup->preset))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return setup->abi_version == RSF_DLSS_ABI_VERSION ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_ABI_MISMATCH;
}

rsf_backend_result create_registration(void* device, const rsf_dlss_setup* setup, void* engine_module,
    rsf_dlss_native12_registration** out)
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    const auto result = validate_setup(device, setup);
    if (result != RSF_BACKEND_OK) return result;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_register(device, setup, engine_module, out, &platform);
#else
    (void)engine_module;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
}

rsf_backend_result rsf_dlss_native12_registration_create(void* device, const rsf_dlss_setup* setup,
    rsf_dlss_native12_registration** out)
{
    return create_registration(device, setup, nullptr, out);
}

rsf_backend_result rsf_dlss_native12_registration_borrow(void* device, void* module,
    const rsf_dlss_setup* setup, rsf_dlss_native12_registration** out)
{
    if (!module) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return create_registration(device, setup, module, out);
}

rsf_backend_result rsf_dlss_native12_create_registered(void* device, const rsf_dlss_setup* setup,
    rsf_dlss_native12_registration* registration, rsf_dlss_native12** out)
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || !registration || !rsf_dlss_preset_valid(setup->preset))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_create_registered(device, setup, registration, out);
#else
    (void)device;
    return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#endif
}

rsf_backend_result rsf_dlss_native12_registration_present(rsf_dlss_native12_registration* registration,
    void* swapchain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    if (!registration || !swapchain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_registration_present(registration, swapchain, interval, flags, begin);
#else
    (void)interval; (void)flags; (void)begin;
    return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#endif
}

void rsf_dlss_native12_registration_destroy(rsf_dlss_native12_registration* registration)
{
#if RSF_HAVE_STREAMLINE
    rsf_core_dlss_native12_registration_destroy(registration);
#else
    (void)registration;
#endif
}

rsf_backend_result rsf_dlss_native12_create(void* device, const rsf_dlss_setup* setup, rsf_dlss_native12** out)
{
    return rsf_dlss_native12_create_shared(device, setup, nullptr, out);
}

rsf_backend_result rsf_dlss_native12_create_shared(void* device, const rsf_dlss_setup* setup,
    rsf_streamline_host* host, rsf_dlss_native12** out)
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    const auto result = validate_setup(device, setup);
    if (result != RSF_BACKEND_OK) return result;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_create(device, setup, host, out, &platform);
#else
    (void)host;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

rsf_backend_result rsf_dlss_native12_plan(rsf_dlss_native12* context, uint32_t width, uint32_t height,
    rsf_quality quality, uint32_t* render_width, uint32_t* render_height)
{
    if (!context || !width || !height || !render_width || !render_height || quality > RSF_QUALITY_ULTRA_PERFORMANCE)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_plan(context, width, height, quality, render_width, render_height);
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

rsf_backend_result rsf_dlss_native12_evaluate(rsf_dlss_native12* context, void* list, const rsf_sr_frame* frame)
{
    if (!context || !list) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_evaluate(context, list, frame, nullptr);
#else
    (void)frame;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

rsf_backend_result rsf_dlss_native12_evaluate_borrowed(rsf_dlss_native12* context, void* list,
    const rsf_sr_frame* frame, void* native_token)
{
    if (!context || !list || !native_token) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_evaluate(context, list, frame, native_token);
#else
    (void)frame;
    return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#endif
}

void rsf_dlss_native12_destroy(rsf_dlss_native12* context)
{
#if RSF_HAVE_STREAMLINE
    rsf_core_dlss_native12_destroy(context);
#else
    (void)context;
#endif
}

void rsf_dlss_native12_set_sdr(rsf_dlss_native12* context, uint32_t sdr)
{
#if RSF_HAVE_STREAMLINE
    if (context) rsf_core_dlss_native12_set_sdr(context, sdr);
#else
    (void)context; (void)sdr;
#endif
}

rsf_backend_result rsf_dlss_native12_present(rsf_dlss_native12* context, void* swapchain,
    uint32_t sync_interval, uint32_t flags, uint32_t begin)
{
    if (!context || !swapchain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if RSF_HAVE_STREAMLINE
    return rsf_core_dlss_native12_present(context, swapchain, sync_interval, flags, begin);
#else
    (void)sync_interval; (void)flags; (void)begin;
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
