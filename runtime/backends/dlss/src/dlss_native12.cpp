// SPDX-License-Identifier: MIT
#include <rescaleframe/dlss_native12.h>

#if RSF_HAVE_STREAMLINE
#include "streamline_frame.h"
#include "streamline_sr.h"
#include "streamline_load.h"
#include <sl_consts.h>
#include <DirectXMath.h>
#include <dxgi.h>
#include <cstddef>
#include <cstdio>
#include <cstring>

using CoreEntries = void*[10];
using FeatureEntries = void*[4];
using FeatureLookup = int32_t (*)(void*, uint32_t, const char*, void**);
struct Preferences {
    uint64_t flags;
    const char* engine_version;
    const char* project_id;
    uint32_t engine, application_id, render_api, feature;
};
struct ConstantsInput {
    const rsf_sr_frame* frame;
    float previous_to_current[16], motion_scale[2], forward[3], aspect, invalid_motion;
};
static_assert(sizeof(Preferences) == 40 && sizeof(ConstantsInput) == 104 &&
    offsetof(ConstantsInput, aspect) == 92);
static_assert(uint64_t(sl::PreferenceFlags::eUseManualHooking) == (1ull << 2) &&
    uint64_t(sl::PreferenceFlags::eUseFrameBasedResourceTagging) == (1ull << 7) &&
    uint32_t(sl::EngineType::eCustom) == 0 && uint32_t(sl::EngineType::eUnity) == RSF_DLSS_ENGINE_UNITY &&
    uint32_t(sl::RenderAPI::eD3D12) == 1 && sl::kFeatureDLSS == 0 && uint32_t(sl::Result::eOk) == 0);

namespace {
// Typed SDK entry points and preferences. The Rust registration leases the module
// and keeps the borrowed plugin directory alive.
struct Native {
    const wchar_t* plugin_paths[1]{};
    sl::Feature feature_id{};
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

void* native_allocate(const wchar_t* directory) noexcept try
{
    auto* native = new Native{};
    native->plugin_paths[0] = directory;
    return native;
} catch (...) { return nullptr; }

void native_bind_core(void* handle, const CoreEntries* entries) noexcept
{
    auto* native = static_cast<Native*>(handle);
    const auto& [init, set_device, support, feature, shutdown, token, constants, tags, evaluate,
        free_resources] = *entries;
    native->init = reinterpret_cast<PFun_slInit*>(init);
    native->set_device = reinterpret_cast<PFun_slSetD3DDevice*>(set_device);
    native->support = reinterpret_cast<PFun_slIsFeatureSupported*>(support);
    native->feature = reinterpret_cast<PFun_slGetFeatureFunction*>(feature);
    native->shutdown = reinterpret_cast<PFun_slShutdown*>(shutdown);
    native->token = reinterpret_cast<PFun_slGetNewFrameToken*>(token);
    native->constants = reinterpret_cast<PFun_slSetConstants*>(constants);
    native->tags = reinterpret_cast<PFun_slSetTagForFrame*>(tags);
    native->evaluate = reinterpret_cast<PFun_slEvaluateFeature*>(evaluate);
    native->free_resources = reinterpret_cast<PFun_slFreeResources*>(free_resources);
}

rsf_backend_result native_init(void* handle, const Preferences* input) noexcept try
{
    auto& native = *static_cast<Native*>(handle);
    native.feature_id = static_cast<sl::Feature>(input->feature);
    sl::Preferences preferences{};
    preferences.renderAPI = static_cast<sl::RenderAPI>(input->render_api);
    preferences.flags = static_cast<sl::PreferenceFlags>(input->flags);
    preferences.featuresToLoad = &native.feature_id;
    preferences.numFeaturesToLoad = 1;
    preferences.pathsToPlugins = native.plugin_paths;
    preferences.numPathsToPlugins = 1;
    preferences.engine = static_cast<sl::EngineType>(input->engine);
    preferences.applicationId = input->application_id;
    preferences.engineVersion = input->engine_version;
    preferences.projectId = input->project_id;
    return native.init(preferences, sl::kSDKVersion) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_set_device(void* handle, void* device) noexcept try
{
    return static_cast<Native*>(handle)->set_device(device) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t lookup_feature(void* handle, uint32_t group, const char* name, void** out) noexcept try
{
    const auto feature = group == 0 ? sl::kFeatureDLSS : sl::kFeatureCommon;
    return static_cast<Native*>(handle)->feature(feature, name, *out) == sl::Result::eOk
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_support(void* handle, void* device, uint32_t* result) noexcept try
{
    auto& native = *static_cast<Native*>(handle);
    LUID luid = static_cast<ID3D12Device*>(device)->GetAdapterLuid();
    sl::AdapterInfo adapter{};
    adapter.deviceLUID = reinterpret_cast<uint8_t*>(&luid);
    adapter.deviceLUIDSizeInBytes = sizeof(luid);
    *result = static_cast<uint32_t>(native.support(sl::kFeatureDLSS, adapter));
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void native_bind_features(void* handle, const FeatureEntries* entries, uint32_t owns_present) noexcept
{
    auto& native = *static_cast<Native*>(handle);
    const auto& [optimal, options, before, after] = *entries;
    native.optimal = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal);
    native.options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    if (owns_present) {
        native.present = reinterpret_cast<decltype(native.present)>(before);
        native.after_present = reinterpret_cast<decltype(native.after_present)>(after);
    }
}

rsf_backend_result native_report_support(const rsf_dlss_setup* setup, uint32_t result) noexcept try
{
    if (setup->log) {
        if (result == 0) {
            setup->log(setup->log_user, "DLSS SR registered on the caller's native D3D12 device.");
        } else {
            char message[128];
            std::snprintf(message, sizeof(message), "DLSS native D3D12 support query returned Streamline result %u", result);
            setup->log(setup->log_user, message);
        }
    }
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

rsf_backend_result native_shutdown(void* handle) noexcept try
{
    return static_cast<Native*>(handle)->shutdown() == sl::Result::eOk
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

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

rsf_backend_result native_plan(void* handle, uint32_t width, uint32_t height, uint32_t mode,
    uint32_t* render_width, uint32_t* render_height) noexcept try
{
    sl::DLSSOptions options{};
    options.mode = static_cast<sl::DLSSMode>(mode);
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

float native_inverse(const float (*matrix)[16], float (*inverse)[16]) noexcept
{
    DirectX::XMFLOAT4X4 copied{}, inverted{};
    std::memcpy(&copied, *matrix, sizeof(copied));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverted, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&copied)));
    std::memcpy(*inverse, &inverted, sizeof(inverted));
    return DirectX::XMVectorGetX(determinant);
}

rsf_backend_result native_constants(void* handle, void* token_ptr, uint32_t viewport_id,
    const ConstantsInput* input) noexcept try
{
    const auto* frame = input->frame;
    const auto& native = *static_cast<Native*>(handle);
    const auto* token = static_cast<sl::FrameToken*>(token_ptr);
    const sl::ViewportHandle viewport(viewport_id);
    const auto& record = *frame->record;
    const auto& camera = record.camera;
    sl::Constants constants{};
    constants.cameraViewToClip = rsf::sl_matrix(camera.view_to_clip);
    constants.clipToCameraView = rsf::sl_matrix(camera.clip_to_view);
    constants.clipToPrevClip = rsf::sl_matrix(camera.clip_to_previous_clip);
    constants.prevClipToClip = rsf::sl_matrix(input->previous_to_current);
    constants.jitterOffset = {frame->jitter_x, frame->jitter_y};
    constants.mvecScale = {input->motion_scale[0], input->motion_scale[1]};
    constants.cameraPinholeOffset = {0, 0};
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {input->forward[0], input->forward[1], input->forward[2]};
    constants.cameraNear = camera.near_plane;
    constants.cameraFar = camera.far_plane;
    constants.cameraFOV = camera.vertical_fov_radians;
    constants.cameraAspectRatio = input->aspect;
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue;
    constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = input->invalid_motion;
    constants.reset = frame->reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    return native.constants(constants, *token, viewport) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_options(void* handle, uint32_t viewport, const rsf_streamline_sr_options* input) noexcept try
{
    const auto& native = *static_cast<Native*>(handle);
    sl::DLSSOptions options{};
    options.mode = static_cast<sl::DLSSMode>(input->mode);
    const auto preset = static_cast<sl::DLSSPreset>(input->preset);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = preset;
    options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    options.outputWidth = input->width;
    options.outputHeight = input->height;
    options.colorBuffersHDR = input->hdr ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    options.useAutoExposure = input->auto_exposure ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    return native.options(sl::ViewportHandle(viewport), options) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result native_dispatch(void* handle, void* token_ptr, uint32_t viewport_id, void* list,
    const rsf_streamline_sr_tags* tags) noexcept try
{
    const auto& native = *static_cast<Native*>(handle);
    const auto* token = static_cast<sl::FrameToken*>(token_ptr);
    const sl::ViewportHandle viewport(viewport_id);
    if (rsf::set_sr_tags(native.tags, *token, viewport, *tags, static_cast<sl::CommandBuffer*>(list)) != sl::Result::eOk)
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
    decltype(&native_allocate) allocate;
    decltype(&native_bind_core) bind_core;
    decltype(&native_init) init;
    decltype(&native_set_device) set_device;
    decltype(&native_support) support;
    decltype(&lookup_feature) lookup_feature;
    decltype(&native_bind_features) bind_features;
    decltype(&native_report_support) report_support;
    decltype(&native_shutdown) shutdown;
    decltype(&native_close) close;
    decltype(&native_present) present;
    decltype(&native_plan) plan;
    decltype(&native_validate) validate;
    decltype(&native_token) token;
    decltype(&native_shared_token) shared_token;
    decltype(&native_inverse) inverse;
    decltype(&native_constants) constants;
    decltype(&native_options) options;
    decltype(&native_dispatch) dispatch;
    decltype(&native_free_viewport) free_viewport;
};
static_assert(sizeof(Platform) == 23 * sizeof(void*));
static_assert(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 && D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 8);
static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_BACKEND_ERROR_ABI_MISMATCH == -2 && RSF_BACKEND_ERROR_LOAD_FAILED == -4 && RSF_BACKEND_ERROR_INIT_FAILED == -6 &&
    RSF_BACKEND_ERROR_NOT_READY == -8 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9 &&
    RSF_QUALITY_QUALITY == 1);

const Platform platform{
    native_load, native_verify, native_shared_module, native_allocate, native_bind_core,
    native_init, native_set_device, native_support, lookup_feature, native_bind_features, native_report_support,
    native_shutdown, native_close, native_present,
    native_plan, native_validate, native_token, native_shared_token, native_inverse, native_constants, native_options,
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
