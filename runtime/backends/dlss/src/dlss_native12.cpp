// SPDX-License-Identifier: MIT
#include <rescaleframe/dlss_native12.h>

#if RSF_HAVE_STREAMLINE
#include "streamline_frame.h"
#include "streamline_sr.h"
#include <sl_consts.h>
#include <DirectXMath.h>
#include <d3d12.h>
#include <dxgi.h>
#include <cstddef>
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
    uint32_t(sl::EngineType::eCustom) == 0 && uint32_t(sl::EngineType::eUnreal) == RSF_DLSS_ENGINE_UNREAL &&
    uint32_t(sl::EngineType::eUnity) == RSF_DLSS_ENGINE_UNITY &&
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

extern "C" {
void* rsf_dlss_sr_load_sdk(const wchar_t*, const rsf_dlss_setup*);
bool rsf_dlss_sdk_verify_signature(const wchar_t*) noexcept;
int32_t rsf_dlss_sr_shared_module(void*, void*, void**);
int32_t rsf_dlss_sr_validate_d3d12(void*, const rsf_sr_frame*);
}

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

rsf_backend_result native_support(void* handle, uint64_t luid, uint32_t* result) noexcept try
{
    auto& native = *static_cast<Native*>(handle);
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

// Preserve the DirectXMath inverse and determinant used by the SR singularity gate.
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
    const auto options = rsf::sr_options(*input);
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
    decltype(&rsf_dlss_sr_load_sdk) load;
    decltype(&rsf_dlss_sdk_verify_signature) verify;
    decltype(&rsf_dlss_sr_shared_module) shared_module;
    decltype(&native_allocate) allocate;
    decltype(&native_bind_core) bind_core;
    decltype(&native_init) init;
    decltype(&native_set_device) set_device;
    decltype(&native_support) support;
    decltype(&lookup_feature) lookup_feature;
    decltype(&native_bind_features) bind_features;
    decltype(&native_shutdown) shutdown;
    decltype(&native_close) close;
    decltype(&native_present) present;
    decltype(&native_plan) plan;
    decltype(&rsf_dlss_sr_validate_d3d12) validate;
    decltype(&native_token) token;
    decltype(&native_shared_token) shared_token;
    decltype(&native_inverse) inverse;
    decltype(&native_constants) constants;
    decltype(&native_options) options;
    decltype(&native_dispatch) dispatch;
    decltype(&native_free_viewport) free_viewport;
};
static_assert(sizeof(Platform) == 22 * sizeof(void*));
static_assert(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 && D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 8);
static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_BACKEND_ERROR_ABI_MISMATCH == -2 && RSF_BACKEND_ERROR_LOAD_FAILED == -4 && RSF_BACKEND_ERROR_INIT_FAILED == -6 &&
    RSF_BACKEND_ERROR_NOT_READY == -8 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9 &&
    RSF_QUALITY_QUALITY == 1);

const Platform platform{
    rsf_dlss_sr_load_sdk, rsf_dlss_sdk_verify_signature, rsf_dlss_sr_shared_module, native_allocate, native_bind_core,
    native_init, native_set_device, native_support, lookup_feature, native_bind_features,
    native_shutdown, native_close, native_present,
    native_plan, rsf_dlss_sr_validate_d3d12, native_token, native_shared_token, native_inverse, native_constants, native_options,
    native_dispatch, native_free_viewport,
};
}
#endif

extern "C" const void* rsf_dlss_native12_sdk_platform() noexcept
{
#if RSF_HAVE_STREAMLINE
    return &platform;
#else
    return nullptr;
#endif
}
