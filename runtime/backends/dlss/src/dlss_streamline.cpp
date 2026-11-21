// SPDX-License-Identifier: MIT

#include <rescaleframe/dlss.h>
#include <rescaleframe/streamline_host.h>
#include "streamline_diagnostics.h"
#include "streamline_frame.h"
#include "streamline_load.h"

#if RSF_HAVE_STREAMLINE

#include <windows.h>

#include <d3d11.h>
#include <d3d12.h>
#include <dxgi.h>
#include <wrl/client.h>

#include <sl.h>
#include <sl_consts.h>
#include <sl_dlss.h>

// sl_security.h includes <Softpub.h>, which mingw-w64 ships as <softpub.h>. Windows filesystems do
// not care and the reference MSVC build gets the real check; the cross build compiles without it
// and refuses to load rather than pretending it verified anything.
#if __has_include(<Softpub.h>)
#include <sl_security.h>
#define RSF_HAVE_SIGNATURE_CHECK 1
#else
#define RSF_HAVE_SIGNATURE_CHECK 0
#endif

#include <cstddef>
#include <cstdio>

struct rsf_streamline_sr_options {
    uint32_t quality, width, height, hdr, auto_exposure, alpha, preset;
};

using rsf_streamline_sr_entry_addresses = void*[12];

struct rsf_streamline_sr_preferences {
    const wchar_t* plugins;
    const wchar_t* logs;
    const char* engine_version;
    const char* project_id;
    uint64_t flags;
    uint32_t engine, application_id, render_api, log_level, show_console, feature;
};

struct rsf_streamline_sr_startup {
    void* (*allocate)();
    void (*dispose)(void*);
    void (*set_log)(rsf_dlss_log_fn, void*);
    void (*missing_entry)(const char*);
    void (*bind_entries)(void*, const rsf_streamline_sr_entry_addresses*);
    int32_t (*init)(void*, const rsf_streamline_sr_preferences*);
    void* (*host_module)(void*);
    void* (*host_device)(void*);
    int32_t (*register_device)(void*, void*);
    int32_t (*lookup_feature)(void*, const char*, void**);
    void (*bind_features)(void*, void*, void*);
    void (*report)(const char*);
};

struct rsf_streamline_sr_platform {
    void* (*load)(const wchar_t*, const rsf_dlss_setup*);
    const rsf_streamline_sr_startup* startup;
    int32_t (*support)(void*, void*, uint32_t, rsf_dlss_support*);
    int32_t (*plan)(void*, rsf_dlss_plan*);
    int32_t (*present)(void*, void*, uint32_t, uint32_t, uint32_t);
    void* (*token)(void*, void*, uint64_t, uint32_t);
    int32_t (*constants)(void*, void*, void*, uint64_t, uint32_t, const rsf_dlss_frame*);
    int32_t (*options)(void*, uint32_t, const rsf_streamline_sr_options*);
    int32_t (*tags)(void*, void*, void*, uint32_t, uint32_t, const rsf_dlss_frame*);
    int32_t (*evaluate)(void*, void*, void*, uint32_t);
    uint32_t (*drain)(void*);
    int32_t (*release_viewport)(void*, uint32_t);
    int32_t (*shutdown)(void*);
    void (*destroy)(void*);
};

#endif
}

namespace {

// Everything Streamline exports that this integration uses. Resolved by name from the interposer
// rather than linked, because the game process must keep working when the SDK is not deployed,
// and because a missing entry point is then a reported failure instead of a load-time abort.
struct Entries {
    PFun_slInit* init = nullptr;
    PFun_slShutdown* shutdown = nullptr;
    PFun_slIsFeatureSupported* is_feature_supported = nullptr;
    PFun_slSetD3DDevice* set_device = nullptr;
    PFun_slGetFeatureFunction* get_feature_function = nullptr;
    PFun_slGetFeatureRequirements* get_feature_requirements = nullptr;
    PFun_slGetNewFrameToken* get_new_frame_token = nullptr;
    PFun_slSetConstants* set_constants = nullptr;
    PFun_slSetTagForFrame* set_tag_for_frame = nullptr;
    PFun_slEvaluateFeature* evaluate_feature = nullptr;
    PFun_slAllocateResources* allocate_resources = nullptr;
    PFun_slFreeResources* free_resources = nullptr;
};

struct Native {
    ~Native()
    {
        rsf_core_streamline_log_clear(RSF_STREAMLINE_LOG_LEGACY_SR, 0);
    }
    Entries sl{};
    PFun_slDLSSGetOptimalSettings* get_optimal_settings = nullptr;
    PFun_slDLSSSetOptions* set_options = nullptr;
    HRESULT (*present)(IDXGISwapChain*, UINT, UINT, bool&){};
    HRESULT (*after_present)(UINT){};

    // The SDK borrows this array and Rust owns the strings through shutdown.
    const wchar_t* plugin_paths[1]{};
    sl::Feature feature{};
};

void set_log(rsf_dlss_log_fn callback, void* user) noexcept
{
    if (callback) {
        rsf_core_streamline_log_set(RSF_STREAMLINE_LOG_LEGACY_SR, 0, callback, user, 0);
    } else {
        rsf_core_streamline_log_clear(RSF_STREAMLINE_LOG_LEGACY_SR, 0);
    }
}

template <typename... Arguments>
void say(const char* format, Arguments... arguments) noexcept try
{
    char text[512]{};
    if constexpr (sizeof...(arguments) == 0) {
        std::snprintf(text, sizeof(text), "%s", format);
    } else {
        std::snprintf(text, sizeof(text), format, arguments...);
    }
    rsf_core_streamline_log_emit(RSF_STREAMLINE_LOG_LEGACY_SR, 1, text, rsf::emit_streamline_log);
} catch (...) {}

// Streamline's own log messages, forwarded to the same sink rather than to a console nobody sees.
void streamline_message(sl::LogType type, const char* message)
{
    // NGX's per-resource info messages grew the research log by hundreds of MB per session.
    // Keep actionable vendor diagnostics; our own state transitions already describe startup.
    if (type != sl::LogType::eError && type != sl::LogType::eWarn) {
        return;
    }
    say("streamline %s: %s", type == sl::LogType::eError ? "error" : "warning", message ? message : "");
}

void missing_entry(const char* name) noexcept
{
    say("entry point %s is missing from the interposer", name);
}

void bind_entries(void* native, const rsf_streamline_sr_entry_addresses* addresses) noexcept
{
    auto& entries = static_cast<Native*>(native)->sl;
    const auto& [is_feature_supported, get_feature_function, get_feature_requirements, set_constants,
        set_tag_for_frame, evaluate_feature, free_resources, init, shutdown, set_device,
        get_new_frame_token, allocate_resources] = *addresses;
    entries.is_feature_supported = reinterpret_cast<PFun_slIsFeatureSupported*>(is_feature_supported);
    entries.get_feature_function = reinterpret_cast<PFun_slGetFeatureFunction*>(get_feature_function);
    entries.get_feature_requirements = reinterpret_cast<PFun_slGetFeatureRequirements*>(get_feature_requirements);
    entries.set_constants = reinterpret_cast<PFun_slSetConstants*>(set_constants);
    entries.set_tag_for_frame = reinterpret_cast<PFun_slSetTagForFrame*>(set_tag_for_frame);
    entries.evaluate_feature = reinterpret_cast<PFun_slEvaluateFeature*>(evaluate_feature);
    entries.free_resources = reinterpret_cast<PFun_slFreeResources*>(free_resources);
    entries.init = reinterpret_cast<PFun_slInit*>(init);
    entries.shutdown = reinterpret_cast<PFun_slShutdown*>(shutdown);
    entries.set_device = reinterpret_cast<PFun_slSetD3DDevice*>(set_device);
    entries.get_new_frame_token = reinterpret_cast<PFun_slGetNewFrameToken*>(get_new_frame_token);
    entries.allocate_resources = reinterpret_cast<PFun_slAllocateResources*>(allocate_resources);
}

sl::Boolean flag(uint32_t value)
{
    return value ? sl::Boolean::eTrue : sl::Boolean::eFalse;
}

const sl::ViewportHandle& viewport_handle(uint32_t slot)
{
    static const sl::ViewportHandle handles[] = {sl::ViewportHandle{0u}, sl::ViewportHandle{1u}};
    return handles[slot];
}

int32_t lookup_dlss_function(void* native, const char* name, void** address) noexcept try
{
    const auto result = static_cast<Native*>(native)->sl.get_feature_function(sl::kFeatureDLSS, name, *address);
    return result == sl::Result::eOk ? RSF_DLSS_OK : RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

void bind_dlss_functions(void* native, void* optimal, void* options) noexcept
{
    auto& self = *static_cast<Native*>(native);
    self.get_optimal_settings = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal);
    self.set_options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
}

void* load_owned(const wchar_t* path, const rsf_dlss_setup* setup) noexcept try
{
    return rsf::load_interposer(path, setup->require_signature != 0, setup->log, setup->log_user);
} catch (...) { return nullptr; }

void* allocate_native() noexcept try
{
    return new Native();
} catch (...) { return nullptr; }

void dispose_native(void* native) noexcept
{
    delete static_cast<Native*>(native);
}

int32_t init_owned(void* native, const rsf_streamline_sr_preferences* input) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    sl::Preferences preferences{};
    preferences.flags |= static_cast<sl::PreferenceFlags>(input->flags);
    preferences.renderAPI = static_cast<sl::RenderAPI>(input->render_api);
    preferences.logLevel = static_cast<sl::LogLevel>(input->log_level);
    preferences.showConsole = input->show_console != 0;
    preferences.logMessageCallback = streamline_message;
    self.feature = static_cast<sl::Feature>(input->feature);
    preferences.featuresToLoad = &self.feature;
    preferences.numFeaturesToLoad = 1;
    if (input->plugins) {
        self.plugin_paths[0] = input->plugins;
        preferences.pathsToPlugins = self.plugin_paths;
        preferences.numPathsToPlugins = 1;
    }
    preferences.pathToLogsAndData = input->logs;
    preferences.applicationId = input->application_id;
    preferences.engine = static_cast<sl::EngineType>(input->engine);
    preferences.engineVersion = input->engine_version;
    preferences.projectId = input->project_id;
    const auto result = self.sl.init(preferences, sl::kSDKVersion);
    if (result != sl::Result::eOk) {
        say("slInit failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_INIT_FAILED;
    }
    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_INIT_FAILED; }

void* host_module(void* raw_host) noexcept
{
    return rsf_streamline_host_module(static_cast<rsf_streamline_host*>(raw_host));
}

void* host_device(void* raw_host) noexcept
{
    rsf_streamline_graphics graphics{};
    graphics.struct_size = sizeof(graphics);
    return rsf_streamline_host_graphics(static_cast<rsf_streamline_host*>(raw_host), &graphics) == RSF_BACKEND_OK
        ? graphics.native_device : nullptr;
}

int32_t register_device(void* native, void* device) noexcept try
{
    const auto result = static_cast<Native*>(native)->sl.set_device(device);
    if (result != sl::Result::eOk) {
        say("slSetD3DDevice failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t present(void* native, void* swapchain, uint32_t interval, uint32_t flags, uint32_t begin) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    if (!self.present || !self.after_present) {
        void* before = nullptr; void* after = nullptr;
        if (self.sl.get_feature_function(sl::kFeatureCommon, "slHookPresent", before) != sl::Result::eOk ||
            self.sl.get_feature_function(sl::kFeatureCommon, "slHookAfterPresent", after) != sl::Result::eOk || !before || !after)
            return RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
        self.present = reinterpret_cast<decltype(self.present)>(before);
        self.after_present = reinterpret_cast<decltype(self.after_present)>(after);
    }
    bool skip = false;
    const auto result = begin ? self.present(static_cast<IDXGISwapChain*>(swapchain), interval, flags, skip) : self.after_present(flags);
    return SUCCEEDED(result) && !skip ? RSF_DLSS_OK : RSF_DLSS_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t query_support(void* native, void* device, uint32_t shared, rsf_dlss_support* support) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    // The adapter comes from the game's own device. Enumerating adapters instead would answer for
    // a GPU the game is not rendering on, which on a laptop is the usual case rather than a rare
    // one.
    DXGI_ADAPTER_DESC description{};
    if (shared) {
        description.AdapterLuid = static_cast<ID3D12Device*>(device)->GetAdapterLuid();
    } else {
        Microsoft::WRL::ComPtr<IDXGIDevice> dxgi_device;
        Microsoft::WRL::ComPtr<IDXGIAdapter> adapter;
        if (FAILED(static_cast<ID3D11Device*>(device)->QueryInterface(IID_PPV_ARGS(&dxgi_device))) ||
            FAILED(dxgi_device->GetAdapter(&adapter)) || !adapter) {
            return RSF_DLSS_ERROR_NOT_READY;
        }
        if (FAILED(adapter->GetDesc(&description))) {
            return RSF_DLSS_ERROR_NOT_READY;
        }
    }

    sl::AdapterInfo info{};
    info.deviceLUID = reinterpret_cast<uint8_t*>(&description.AdapterLuid);
    info.deviceLUIDSizeInBytes = sizeof(description.AdapterLuid);

    const sl::Result result = self.sl.is_feature_supported(sl::kFeatureDLSS, info);
    support->supported = result == sl::Result::eOk ? 1u : 0u;
    support->driver_out_of_date = result == sl::Result::eErrorDriverOutOfDate ? 1u : 0u;
    support->os_out_of_date = result == sl::Result::eErrorOSOutOfDate ? 1u : 0u;
    support->no_supported_adapter = result == sl::Result::eErrorNoSupportedAdapterFound ? 1u : 0u;

    support->required_driver_major = 0;
    support->required_driver_minor = 0;
    support->detected_driver_major = 0;
    support->detected_driver_minor = 0;
    sl::FeatureRequirements requirements{};
    if (self.sl.get_feature_requirements(sl::kFeatureDLSS, requirements) == sl::Result::eOk) {
        support->required_driver_major = requirements.driverVersionRequired.major;
        support->required_driver_minor = requirements.driverVersionRequired.minor;
        support->detected_driver_major = requirements.driverVersionDetected.major;
        support->detected_driver_minor = requirements.driverVersionDetected.minor;
    }

    const bool supported = support->supported != 0u;
    say("DLSS support on this adapter: %s (streamline result %u, driver %u.%u, requires %u.%u)",
        supported ? "yes" : "no", static_cast<unsigned>(result),
        support->detected_driver_major, support->detected_driver_minor,
        support->required_driver_major, support->required_driver_minor);
    return supported ? RSF_DLSS_OK : RSF_DLSS_ERROR_NOT_SUPPORTED;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t render_plan(void* native, rsf_dlss_plan* plan) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    sl::DLSSOptions options{};
    options.mode = rsf::dlss_mode(plan->quality);
    options.outputWidth = plan->output_width;
    options.outputHeight = plan->output_height;

    sl::DLSSOptimalSettings settings{};
    const sl::Result result = self.get_optimal_settings(options, settings);
    if (result != sl::Result::eOk) {
        say("slDLSSGetOptimalSettings failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    plan->render_width = settings.optimalRenderWidth;
    plan->render_height = settings.optimalRenderHeight;
    plan->render_width_min = settings.renderWidthMin;
    plan->render_height_min = settings.renderHeightMin;
    plan->render_width_max = settings.renderWidthMax;
    plan->render_height_max = settings.renderHeightMax;
    say("DLSS wants %ux%u to produce %ux%u", plan->render_width, plan->render_height,
        plan->output_width, plan->output_height);
    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

void* get_token(void* native, void* host, uint64_t source, uint32_t frame_index) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    if (host) return rsf_streamline_host_token(static_cast<rsf_streamline_host*>(host), source);
    sl::FrameToken* token = nullptr;
    return self.sl.get_new_frame_token(token, frame_index ? &frame_index : nullptr) == sl::Result::eOk
        ? token : nullptr;
} catch (...) { return nullptr; }

int32_t set_constants(void* native, void* host, void* token, uint64_t source, uint32_t slot,
    const rsf_dlss_frame* frame) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    sl::Constants constants{};
    constants.cameraViewToClip = rsf::sl_matrix(frame->camera_view_to_clip);
    constants.clipToCameraView = rsf::sl_matrix(frame->clip_to_camera_view);
    constants.clipToPrevClip = rsf::sl_matrix(frame->clip_to_prev_clip);
    constants.prevClipToClip = rsf::sl_matrix(frame->prev_clip_to_clip);
    constants.jitterOffset = {frame->jitter_x, frame->jitter_y};
    // Zero rather than left alone. Streamline warns that an invalid pinhole offset is a mistake,
    // and the game uses a plain pinhole camera, so zero is the true value rather than a placeholder.
    constants.cameraPinholeOffset = {0.0f, 0.0f};
    constants.mvecScale = {frame->motion_scale_x, frame->motion_scale_y};
    constants.cameraPos = {frame->camera_position[0], frame->camera_position[1],
                           frame->camera_position[2]};
    constants.cameraUp = {frame->camera_up[0], frame->camera_up[1], frame->camera_up[2]};
    constants.cameraRight = {frame->camera_right[0], frame->camera_right[1],
                             frame->camera_right[2]};
    constants.cameraFwd = {frame->camera_forward[0], frame->camera_forward[1],
                           frame->camera_forward[2]};
    constants.cameraNear = frame->near_plane;
    constants.cameraFar = frame->far_plane;
    constants.cameraFOV = frame->vertical_fov;
    constants.cameraAspectRatio = frame->aspect_ratio;
    constants.depthInverted = flag(frame->depth_inverted);
    constants.cameraMotionIncluded = flag(frame->camera_motion_included);
    constants.motionVectors3D = sl::Boolean::eFalse;
    constants.reset = flag(frame->reset);
    // The caller supplies the invalid-motion value for its producer.
    constants.motionVectorsInvalidValue = frame->motion_invalid_value;

    const bool common = host ? rsf_streamline_common_set(static_cast<rsf_streamline_host*>(host), source, frame->viewport, constants) :
        self.sl.set_constants(constants, *static_cast<sl::FrameToken*>(token), viewport_handle(slot)) == sl::Result::eOk;
    if (!common) {
        say("slSetConstants failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t set_options(void* native, uint32_t slot, const rsf_streamline_sr_options* input) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    if (!rsf_dlss_preset_valid(input->preset)) return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    sl::DLSSOptions options{};
    const auto preset = static_cast<sl::DLSSPreset>(input->preset);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = preset;
    options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    options.mode = rsf::dlss_mode(input->quality);
    options.outputWidth = input->width;
    options.outputHeight = input->height;
    options.colorBuffersHDR = flag(input->hdr);
    options.useAutoExposure = flag(input->auto_exposure);
    options.alphaUpscalingEnabled = flag(input->alpha);
    if (self.set_options(viewport_handle(slot), options) != sl::Result::eOk) {
        say("slDLSSSetOptions failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t set_tags(void* native, void* context, void* token, uint32_t slot, uint32_t shared,
    const rsf_dlss_frame* frame) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    const sl::Extent render_extent{0, 0, frame->render_width, frame->render_height};
    const sl::Extent output_extent{0, 0, frame->output_width, frame->output_height};
    const sl::Extent exposure_extent{0, 0, 1, 1};

    sl::Resource color_in{sl::ResourceType::eTex2d, frame->color_in, nullptr, nullptr, 0};
    sl::Resource color_out{sl::ResourceType::eTex2d, frame->color_out, nullptr, nullptr, 0};
    sl::Resource depth{sl::ResourceType::eTex2d, frame->depth, nullptr, nullptr, 0};
    sl::Resource motion{sl::ResourceType::eTex2d, frame->motion, nullptr, nullptr, 0};
    sl::Resource exposure{sl::ResourceType::eTex2d, frame->exposure, nullptr, nullptr, 0};
    // Optional translucency hints, in the order Streamline names them. DLSS SR reads the
    // transparency and bias hints; the other two are Ray Reconstruction inputs.
    struct { void* texture; sl::BufferType type; } const hint_inputs[] = {
        {frame->transparency_hint, sl::kBufferTypeTransparencyHint},
        {frame->bias_current_color, sl::kBufferTypeBiasCurrentColorHint},
        {frame->color_before_transparency, sl::kBufferTypeColorBeforeTransparency},
        {frame->transparency_layer, sl::kBufferTypeTransparencyLayer},
    };
    sl::Resource hints[4]{};
    for (uint32_t i = 0; i < 4; ++i) {
        hints[i] = sl::Resource{sl::ResourceType::eTex2d, hint_inputs[i].texture, nullptr, nullptr, 0};
        if (shared) hints[i].state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    if (shared) {
        color_in.state = depth.state = motion.state = exposure.state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        color_out.state = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    }

    // Everything is tagged as valid only now. These are engine scene targets that the game reuses
    // later in the same frame, and claiming otherwise would hand DLSS a buffer holding something
    // else by the time it reads it.
    sl::ResourceTag tags[9] = {
        sl::ResourceTag{&color_in, sl::kBufferTypeScalingInputColor,
                        sl::ResourceLifecycle::eOnlyValidNow, &render_extent},
        sl::ResourceTag{&color_out, sl::kBufferTypeScalingOutputColor,
                        sl::ResourceLifecycle::eOnlyValidNow, &output_extent},
        sl::ResourceTag{&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow,
                        &render_extent},
        sl::ResourceTag{&motion, sl::kBufferTypeMotionVectors,
                        sl::ResourceLifecycle::eOnlyValidNow, &render_extent},
    };
    uint32_t tag_count = 4u;
    if (frame->exposure) tags[tag_count++] = sl::ResourceTag{&exposure, sl::kBufferTypeExposure,
        sl::ResourceLifecycle::eOnlyValidNow, &exposure_extent};
    for (uint32_t i = 0; i < 4; ++i) if (hint_inputs[i].texture)
        tags[tag_count++] = sl::ResourceTag{&hints[i], hint_inputs[i].type, sl::ResourceLifecycle::eOnlyValidNow, &render_extent};

    if (self.sl.set_tag_for_frame(*static_cast<sl::FrameToken*>(token), viewport_handle(slot),
        tags, tag_count, static_cast<sl::CommandBuffer*>(context)) !=
        sl::Result::eOk) {
        say("slSetTagForFrame failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t evaluate(void* native, void* context, void* token, uint32_t slot) noexcept try
{
    auto& self = *static_cast<Native*>(native);
    const sl::BaseStructure* inputs[] = {&viewport_handle(slot)};
    const auto result = self.sl.evaluate_feature(sl::kFeatureDLSS, *static_cast<sl::FrameToken*>(token),
        inputs, 1, static_cast<sl::CommandBuffer*>(context));
    if (result != sl::Result::eOk) {
        say("slEvaluateFeature failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

uint32_t drain(void* host) noexcept try
{
    return rsf_streamline_host_drain(static_cast<rsf_streamline_host*>(host)) == RSF_BACKEND_OK;
} catch (...) { return 0; }

int32_t release_viewport(void* native, uint32_t slot) noexcept try
{
    return static_cast<Native*>(native)->sl.free_resources(sl::kFeatureDLSS, viewport_handle(slot)) ==
        sl::Result::eOk ? RSF_DLSS_OK : RSF_DLSS_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t shutdown(void* native) noexcept try
{
    // The legacy route treats a returned shutdown result as terminal.
    static_cast<Native*>(native)->sl.shutdown();
    return RSF_DLSS_OK;
} catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

void destroy(void* native) noexcept
{
    say("streamline shut down");
    dispose_native(native);
}

constexpr rsf_streamline_sr_startup startup{
    allocate_native, dispose_native, set_log, missing_entry, bind_entries, init_owned, host_module, host_device,
    register_device, lookup_dlss_function, bind_dlss_functions, [](const char* text) { say(text); },
};
static_assert(sizeof(rsf_streamline_sr_startup) == 12 * sizeof(void*));
static_assert(sizeof(rsf_streamline_sr_preferences) == 64 && offsetof(rsf_streamline_sr_preferences, flags) == 32);
static_assert(uint64_t(sl::PreferenceFlags::eUseManualHooking) == (1ull << 2) &&
    uint64_t(sl::PreferenceFlags::eUseFrameBasedResourceTagging) == (1ull << 7) &&
    uint32_t(sl::RenderAPI::eD3D11) == 0 && uint32_t(sl::LogLevel::eDefault) == 1 && sl::kFeatureDLSS == 0);
static_assert(uint32_t(sl::EngineType::eCustom) == 0 && uint32_t(sl::EngineType::eUnreal) == RSF_DLSS_ENGINE_UNREAL &&
    uint32_t(sl::EngineType::eUnity) == RSF_DLSS_ENGINE_UNITY);
constexpr rsf_streamline_sr_platform platform{load_owned, &startup, query_support,
    render_plan, present, get_token, set_constants, set_options, set_tags, evaluate, drain,
    release_viewport, shutdown, destroy};
static_assert(sizeof(rsf_streamline_sr_platform) == 14 * sizeof(void*));
static_assert(RSF_DLSS_ERROR_LOAD_FAILED == -4);
static_assert(sizeof(rsf_streamline_sr_options) == 28);
static_assert(sizeof(rsf_dlss_setup) == 88 && sizeof(rsf_dlss_support) == 36 &&
    sizeof(rsf_dlss_plan) == 40 && sizeof(rsf_dlss_frame) == 488);
static_assert(RSF_DLSS_OK == 0 && RSF_DLSS_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_DLSS_ERROR_ABI_MISMATCH == -2 && RSF_DLSS_ERROR_MISSING_ENTRY_POINT == -5 &&
    RSF_DLSS_ERROR_INIT_FAILED == -6 && RSF_DLSS_ERROR_NOT_READY == -8 && RSF_DLSS_ERROR_FEATURE_FAILED == -9);
} // namespace

extern "C" {
int32_t rsf_core_dlss_streamline_load(const rsf_dlss_setup*, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_share_host(void*, rsf_dlss_log_fn, void*, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_set_device(void*, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_support(rsf_dlss_support*, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_plan(rsf_dlss_plan*, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_present(void*, uint32_t, uint32_t, uint32_t, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_evaluate(void*, const rsf_dlss_frame*, uint64_t, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_release(uint32_t, const rsf_streamline_sr_platform*);
int32_t rsf_core_dlss_streamline_shutdown(const rsf_streamline_sr_platform*);

uint32_t rsf_dlss_available(void) { return 1u; }
rsf_dlss_result rsf_dlss_load(const rsf_dlss_setup* setup)
{
    return rsf_core_dlss_streamline_load(setup, &platform);
}
rsf_dlss_result rsf_dlss_set_device(void* device)
{
    return rsf_core_dlss_streamline_set_device(device, &platform);
}
rsf_dlss_result rsf_dlss_native_present(void* chain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    return rsf_core_dlss_streamline_present(chain, interval, flags, begin, &platform);
}
rsf_dlss_result rsf_dlss_query_support(rsf_dlss_support* support)
{
    return rsf_core_dlss_streamline_support(support, &platform);
}
rsf_dlss_result rsf_dlss_plan_render_size(rsf_dlss_plan* plan)
{
    return rsf_core_dlss_streamline_plan(plan, &platform);
}
rsf_dlss_result rsf_dlss_evaluate(void* context, const rsf_dlss_frame* frame)
{
    return rsf_core_dlss_streamline_evaluate(context, frame, 0, &platform);
}
rsf_dlss_result rsf_dlss_evaluate_shared(void* list, const rsf_dlss_frame* frame, uint64_t id)
{
    return id ? rsf_core_dlss_streamline_evaluate(list, frame, id, &platform) : RSF_DLSS_ERROR_NOT_READY;
}
rsf_dlss_result rsf_dlss_share_host(void* host, rsf_dlss_log_fn log, void* user)
{
    return rsf_core_dlss_streamline_share_host(host, log, user, &platform);
}
rsf_dlss_result rsf_dlss_release_viewport(uint32_t index)
{
    return rsf_core_dlss_streamline_release(index, &platform);
}
rsf_dlss_result rsf_dlss_release_resources(void)
{
    return rsf_dlss_release_viewport(0u);
}
rsf_dlss_result rsf_dlss_shutdown(void)
{
    return rsf_core_dlss_streamline_shutdown(&platform);
}
}

#else // RSF_HAVE_STREAMLINE
extern "C" rsf_dlss_result rsf_dlss_native_present(void* chain, uint32_t, uint32_t, uint32_t begin)
{ return chain && begin <= 1 ? RSF_DLSS_ERROR_NOT_COMPILED : RSF_DLSS_ERROR_INVALID_ARGUMENT; }
extern "C" rsf_dlss_result rsf_dlss_share_host(void*, rsf_dlss_log_fn, void*)
{ return RSF_DLSS_ERROR_NOT_COMPILED; }
extern "C" rsf_dlss_result rsf_dlss_evaluate_shared(void*, const rsf_dlss_frame*, uint64_t)
{ return RSF_DLSS_ERROR_NOT_COMPILED; }

// Built without the SDK. The contract still exists so callers compile and can say honestly that
// this build has no DLSS in it, rather than reporting a runtime failure that never happened.

extern "C" rsf_dlss_result rsf_dlss_release_viewport(uint32_t viewport)
{
    (void)viewport;
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

extern "C" uint32_t rsf_dlss_available(void)
{
    return 0u;
}

extern "C" rsf_dlss_result rsf_dlss_load(const rsf_dlss_setup* setup)
{
    if (!setup || setup->struct_size < sizeof(rsf_dlss_setup)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_DLSS_ABI_VERSION) {
        return RSF_DLSS_ERROR_ABI_MISMATCH;
    }
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

extern "C" rsf_dlss_result rsf_dlss_set_device(void* d3d11_device)
{
    return d3d11_device ? RSF_DLSS_ERROR_NOT_COMPILED : RSF_DLSS_ERROR_INVALID_ARGUMENT;
}

extern "C" rsf_dlss_result rsf_dlss_query_support(rsf_dlss_support* support)
{
    if (!support || support->struct_size < sizeof(rsf_dlss_support)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

extern "C" rsf_dlss_result rsf_dlss_plan_render_size(rsf_dlss_plan* plan)
{
    if (!plan || plan->struct_size < sizeof(rsf_dlss_plan)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

extern "C" rsf_dlss_result rsf_dlss_evaluate(void* d3d11_context, const rsf_dlss_frame* frame)
{
    if (!d3d11_context || !frame || frame->struct_size < sizeof(rsf_dlss_frame)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    if (frame->abi_version != RSF_DLSS_ABI_VERSION) {
        return RSF_DLSS_ERROR_ABI_MISMATCH;
    }
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

extern "C" rsf_dlss_result rsf_dlss_release_resources(void)
{
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

extern "C" rsf_dlss_result rsf_dlss_shutdown(void)
{
    return RSF_DLSS_ERROR_NOT_COMPILED;
}

#endif // RSF_HAVE_STREAMLINE
