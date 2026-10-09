// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/dlss.h>
#include <rescaleframe/streamline_host.h>
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

#include <cstdio>
#include <cstring>
#include <string>

// sl_security.h defines globals, so it belongs in exactly one translation unit.
bool rsf_dlss_verify_runtime_signature(const wchar_t* path)
{
#if RSF_HAVE_SIGNATURE_CHECK
    return sl::security::verifyEmbeddedSignature(path);
#else
    (void)path;
    return false;
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

struct State {
    HMODULE interposer = nullptr;
    Entries sl{};
    // Resolved after the device is set, which is when Streamline will hand out feature functions.
    PFun_slDLSSGetOptimalSettings* get_optimal_settings = nullptr;
    PFun_slDLSSSetOptions* set_options = nullptr;
    // What each viewport was last given. Streamline keeps options until they change, so a frame
    // with identical options skips the call.
    sl::DLSSOptions applied_options[2]{};
    bool options_applied[2] = {false, false};

    ID3D11Device* device = nullptr;
    rsf_streamline_host* shared_host = nullptr;
    ID3D12Device* shared_device = nullptr;
    bool initialised = false;

    // Kept alive for the process: Streamline is given pointers to these at init and the
    // documentation does not promise it copies them.
    std::wstring plugin_directory;
    std::wstring log_directory;
    const wchar_t* plugin_paths[1] = {nullptr};

    rsf_dlss_log_fn log = nullptr;
    void* log_user = nullptr;
};

State& state()
{
    static State instance;
    return instance;
}

template <typename... Arguments>
void say(const char* format, Arguments... arguments)
{
    State& self = state();
    if constexpr (sizeof...(arguments) == 0) {
        rsf::say(self.log, self.log_user, "%s", format);
    } else {
        rsf::say(self.log, self.log_user, format, arguments...);
    }
}

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

template <typename T>
bool resolve(HMODULE module, const char* name, T*& target)
{
    target = reinterpret_cast<T*>(reinterpret_cast<void*>(GetProcAddress(module, name)));
    if (!target) {
        say("entry point %s is missing from the interposer", name);
        return false;
    }
    return true;
}

// One resolve chain for both ways in. A Streamline owner loads the interposer itself and needs the
// whole set; a borrower of the shared D3D12 host gets the module from the host, which has already
// initialised it and owns the device and the frame tokens.
bool resolve_entries(HMODULE module, Entries& entries, bool owner)
{
    bool resolved = resolve(module, "slIsFeatureSupported", entries.is_feature_supported) &&
                    resolve(module, "slGetFeatureFunction", entries.get_feature_function) &&
                    resolve(module, "slGetFeatureRequirements", entries.get_feature_requirements) &&
                    resolve(module, "slSetConstants", entries.set_constants) &&
                    resolve(module, "slSetTagForFrame", entries.set_tag_for_frame) &&
                    resolve(module, "slEvaluateFeature", entries.evaluate_feature) &&
                    resolve(module, "slFreeResources", entries.free_resources);
    if (owner) {
        resolved = resolved && resolve(module, "slInit", entries.init) &&
                   resolve(module, "slShutdown", entries.shutdown) &&
                   resolve(module, "slSetD3DDevice", entries.set_device) &&
                   resolve(module, "slGetNewFrameToken", entries.get_new_frame_token) &&
                   resolve(module, "slAllocateResources", entries.allocate_resources);
    }
    return resolved;
}

sl::Boolean flag(uint32_t value)
{
    return value ? sl::Boolean::eTrue : sl::Boolean::eFalse;
}

// The viewports this integration drives: the scene, and a second feature with its own history for
// a layer integrated at one to one. Each has its own tags, constants and options.
uint32_t viewport_slot(uint32_t index)
{
    return index < 2u ? index : 0u;
}

const sl::ViewportHandle& viewport_handle(uint32_t index)
{
    static sl::ViewportHandle handles[2] = {sl::ViewportHandle{0u}, sl::ViewportHandle{1u}};
    return handles[viewport_slot(index)];
}

// The fields this integration sets, which are the ones that can differ between two frames.
bool same_options(const sl::DLSSOptions& a, const sl::DLSSOptions& b)
{
    return a.mode == b.mode && a.outputWidth == b.outputWidth && a.outputHeight == b.outputHeight &&
           a.colorBuffersHDR == b.colorBuffersHDR && a.useAutoExposure == b.useAutoExposure &&
           a.alphaUpscalingEnabled == b.alphaUpscalingEnabled;
}

} // namespace

extern "C" uint32_t rsf_dlss_available(void)
{
    return 1u;
}

extern "C" rsf_dlss_result rsf_dlss_load(const rsf_dlss_setup* setup)
{
    if (!setup || setup->struct_size < sizeof(rsf_dlss_setup) || !setup->interposer_path_utf8) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_DLSS_ABI_VERSION) {
        return RSF_DLSS_ERROR_ABI_MISMATCH;
    }

    State& self = state();
    if (self.initialised) {
        return RSF_DLSS_OK;
    }
    if (GetModuleHandleW(L"sl.interposer.dll")) {
        say("D3D11 DLSS cannot initialize over an existing Streamline owner; use the shared D3D12 host");
        return RSF_DLSS_ERROR_INIT_FAILED;
    }
    self.log = setup->log;
    self.log_user = setup->log_user;

    std::wstring interposer;
    if (!rsf::widen_utf8(setup->interposer_path_utf8, interposer)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }

    // By absolute path, never by name, and verified first when asked: this loads a signed NVIDIA
    // module into a game process from a configured path, and the signature is the difference
    // between that and loading whatever is at that path.
    self.interposer = rsf::load_interposer(interposer.c_str(), setup->require_signature != 0, self.log, self.log_user);
    if (!self.interposer) {
        return RSF_DLSS_ERROR_LOAD_FAILED;
    }

    if (!resolve_entries(self.interposer, self.sl, true)) {
        FreeLibrary(self.interposer);
        self.interposer = nullptr;
        return RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
    }

    sl::Preferences preferences{};
    // Manual hooking is what makes this integration possible at all. The regular mode expects to
    // be in place before the swap chain exists; we attach to a game that is already rendering, and
    // in manual hooking the D3D device may be created before slInit.
    preferences.flags |= sl::PreferenceFlags::eUseManualHooking;
    // Required by slSetTagForFrame, which is what this integration uses: tagging resources against
    // a frame token is what lets Streamline know a tag belongs to the frame being evaluated rather
    // than to whatever was last set. Without the flag the call is refused outright, and the
    // evaluate that follows fails with nothing wrong in the frame itself.
    preferences.flags |= sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.renderAPI = sl::RenderAPI::eD3D11;
    preferences.logLevel = sl::LogLevel::eDefault;
    preferences.showConsole = false;
    preferences.logMessageCallback = streamline_message;

    static const sl::Feature features[] = {sl::kFeatureDLSS};
    preferences.featuresToLoad = features;
    preferences.numFeaturesToLoad = 1;

    if (rsf::widen_utf8(setup->plugin_directory_utf8, self.plugin_directory)) {
        self.plugin_paths[0] = self.plugin_directory.c_str();
        preferences.pathsToPlugins = self.plugin_paths;
        preferences.numPathsToPlugins = 1;
    }
    if (rsf::widen_utf8(setup->log_directory_utf8, self.log_directory)) {
        preferences.pathToLogsAndData = self.log_directory.c_str();
    }
    // NGX will not start without an identity, and DLSS is an NGX feature, so getting this wrong
    // costs the whole thing: the plugin loads and then reports "Missing NGX context". An injected
    // integration has no application id of its own, since that belongs to the game's publisher, so
    // the engine route is the one available. For a UE4 title it is also just true.
    preferences.applicationId = setup->application_id;
    switch (setup->engine) {
    case RSF_DLSS_ENGINE_UNREAL:
        preferences.engine = sl::EngineType::eUnreal;
        break;
    case RSF_DLSS_ENGINE_UNITY:
        preferences.engine = sl::EngineType::eUnity;
        break;
    default:
        preferences.engine = sl::EngineType::eCustom;
        break;
    }
    preferences.engineVersion = setup->engine_version_utf8;
    preferences.projectId = setup->project_id_utf8;

    const sl::Result result = self.sl.init(preferences, sl::kSDKVersion);
    if (result != sl::Result::eOk) {
        say("slInit failed with result %u", static_cast<unsigned>(result));
        FreeLibrary(self.interposer);
        self.interposer = nullptr;
        return RSF_DLSS_ERROR_INIT_FAILED;
    }

    self.initialised = true;
    say("streamline loaded and initialised");
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_set_device(void* d3d11_device)
{
    State& self = state();
    if (self.shared_host) return RSF_DLSS_OK;
    if (!d3d11_device) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    if (!self.initialised) {
        return RSF_DLSS_ERROR_NOT_READY;
    }

    const sl::Result result = self.sl.set_device(d3d11_device);
    if (result != sl::Result::eOk) {
        say("slSetD3DDevice failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    self.device = static_cast<ID3D11Device*>(d3d11_device);
    self.device->AddRef();

    // Feature functions only exist once Streamline knows the device, which is why these are
    // resolved here rather than alongside the core entry points.
    void* optimal = nullptr;
    void* options = nullptr;
    self.sl.get_feature_function(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", optimal);
    self.sl.get_feature_function(sl::kFeatureDLSS, "slDLSSSetOptions", options);
    self.get_optimal_settings = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal);
    self.set_options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    self.options_applied[0] = self.options_applied[1] = false;
    if (!self.get_optimal_settings || !self.set_options) {
        say("DLSS feature functions are not available");
        return RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
    }
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_query_support(rsf_dlss_support* support)
{
    if (!support || support->struct_size < sizeof(rsf_dlss_support)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    State& self = state();
    if (!self.initialised || (!self.device && !self.shared_device)) {
        return RSF_DLSS_ERROR_NOT_READY;
    }

    // The adapter comes from the game's own device. Enumerating adapters instead would answer for
    // a GPU the game is not rendering on, which on a laptop is the usual case rather than a rare
    // one.
    DXGI_ADAPTER_DESC description{};
    if (self.shared_device) {
        description.AdapterLuid = self.shared_device->GetAdapterLuid();
    } else {
        Microsoft::WRL::ComPtr<IDXGIDevice> dxgi_device;
        Microsoft::WRL::ComPtr<IDXGIAdapter> adapter;
        if (FAILED(self.device->QueryInterface(IID_PPV_ARGS(&dxgi_device))) ||
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
}

extern "C" rsf_dlss_result rsf_dlss_plan_render_size(rsf_dlss_plan* plan)
{
    if (!plan || plan->struct_size < sizeof(rsf_dlss_plan) || plan->output_width == 0 ||
        plan->output_height == 0) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    State& self = state();
    if (!self.initialised || !self.get_optimal_settings) {
        return RSF_DLSS_ERROR_NOT_READY;
    }

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
}

static rsf_dlss_result evaluate(void* d3d11_context, const rsf_dlss_frame* frame, uint64_t source_id)
{
    if (!d3d11_context || !frame || frame->struct_size < sizeof(rsf_dlss_frame)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    if (frame->abi_version != RSF_DLSS_ABI_VERSION) {
        return RSF_DLSS_ERROR_ABI_MISMATCH;
    }
    if (!frame->color_in || !frame->color_out || !frame->depth || !frame->motion) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }
    State& self = state();
    if (!self.initialised || (!self.device && !self.shared_device) || !self.set_options) {
        return RSF_DLSS_ERROR_NOT_READY;
    }

    auto* command_buffer = static_cast<sl::CommandBuffer*>(d3d11_context);

    sl::FrameToken* token = nullptr;
    uint32_t frame_index = frame->frame_index;
    if (self.shared_host) {
        token = static_cast<sl::FrameToken*>(rsf_streamline_host_token(self.shared_host, source_id));
        if (!token) return RSF_DLSS_ERROR_NOT_READY;
    } else if (self.sl.get_new_frame_token(token, frame_index ? &frame_index : nullptr) !=
            sl::Result::eOk ||
        !token) {
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    const sl::ViewportHandle& viewport = viewport_handle(frame->viewport);

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
    // The pair that makes an object-only motion buffer usable: Streamline builds camera motion from
    // depth and clipToPrevClip, and this value tells it which pixels nothing wrote. Unreal reserves
    // a raw zero for exactly that, which is why AC7 needs no composition pass for DLSS.
    constants.motionVectorsInvalidValue = frame->motion_invalid_value;

    const bool common = self.shared_host ? rsf_streamline_common_set(self.shared_host, source_id, frame->viewport, constants) :
        self.sl.set_constants(constants, *token, viewport) == sl::Result::eOk;
    if (!common) {
        say("slSetConstants failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    sl::DLSSOptions options{};
    options.mode = rsf::dlss_mode(frame->quality);
    options.outputWidth = frame->output_width;
    options.outputHeight = frame->output_height;
    options.colorBuffersHDR = frame->color_encoded ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    // Auto exposure only when the game does not hand us its own. AC7 keeps one in a 1x1 target and
    // it is bound at the same pass as everything else here, so normally it does.
    options.useAutoExposure = frame->exposure || frame->color_encoded ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    options.alphaUpscalingEnabled = flag(frame->alpha);
    const uint32_t slot = viewport_slot(frame->viewport);
    if (!self.options_applied[slot] || !same_options(self.applied_options[slot], options)) {
        if (self.set_options(viewport, options) != sl::Result::eOk) {
            self.options_applied[slot] = false;
            say("slDLSSSetOptions failed");
            return RSF_DLSS_ERROR_FEATURE_FAILED;
        }
        self.applied_options[slot] = options;
        self.options_applied[slot] = true;
    }

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
        if (self.shared_host) hints[i].state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    if (self.shared_host) {
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

    if (self.sl.set_tag_for_frame(*token, viewport, tags, tag_count, command_buffer) !=
        sl::Result::eOk) {
        say("slSetTagForFrame failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    const sl::BaseStructure* inputs[] = {&viewport};
    const sl::Result result =
        self.sl.evaluate_feature(sl::kFeatureDLSS, *token, inputs, 1, command_buffer);
    if (result != sl::Result::eOk) {
        say("slEvaluateFeature failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_evaluate(void* context, const rsf_dlss_frame* frame)
{
    if (state().shared_host) return RSF_DLSS_ERROR_NOT_READY;
    return evaluate(context, frame, 0);
}
extern "C" rsf_dlss_result rsf_dlss_evaluate_shared(void* list, const rsf_dlss_frame* frame, uint64_t id)
{
    if (!state().shared_host || !id) return RSF_DLSS_ERROR_NOT_READY;
    return evaluate(list, frame, id);
}
extern "C" rsf_dlss_result rsf_dlss_share_host(void* pointer, rsf_dlss_log_fn log, void* user)
{
    auto* host = static_cast<rsf_streamline_host*>(pointer);
    auto& self = state();
    if (!host || self.initialised) return RSF_DLSS_ERROR_NOT_READY;
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    auto module = static_cast<HMODULE>(rsf_streamline_host_module(host));
    if (!module || rsf_streamline_host_graphics(host, &graphics) != RSF_BACKEND_OK)
        return RSF_DLSS_ERROR_NOT_READY;
    Entries entries{};
    if (!resolve_entries(module, entries, false)) return RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
    void* optimal = nullptr; void* options = nullptr;
    if (entries.get_feature_function(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", optimal) != sl::Result::eOk ||
        entries.get_feature_function(sl::kFeatureDLSS, "slDLSSSetOptions", options) != sl::Result::eOk ||
        !optimal || !options) return RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
    self.sl = entries; self.shared_host = host;
    self.shared_device = static_cast<ID3D12Device*>(graphics.native_device);
    self.shared_device->AddRef(); self.initialised = true; self.log = log; self.log_user = user;
    self.get_optimal_settings = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal);
    self.set_options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    self.options_applied[0] = self.options_applied[1] = false;
    say("DLSS SR shares the D3D12 FG host and CPU frame token");
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_release_viewport(uint32_t index)
{
    State& self = state();
    if (!self.initialised) {
        return RSF_DLSS_ERROR_NOT_READY;
    }
    if (self.shared_host && rsf_streamline_host_drain(self.shared_host) != RSF_BACKEND_OK)
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    self.options_applied[viewport_slot(index)] = false;
    if (self.sl.free_resources(sl::kFeatureDLSS, viewport_handle(index)) != sl::Result::eOk) {
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_release_resources(void)
{
    return rsf_dlss_release_viewport(0u);
}

extern "C" rsf_dlss_result rsf_dlss_shutdown(void)
{
    State& self = state();
    if (!self.initialised) {
        return RSF_DLSS_ERROR_NOT_READY;
    }
    if (!self.shared_host) self.sl.shutdown();
    if (self.shared_device) { self.shared_device->Release(); self.shared_device = nullptr; }
    self.shared_host = nullptr;
    if (self.device) {
        self.device->Release();
        self.device = nullptr;
    }
    if (self.interposer) {
        FreeLibrary(self.interposer);
        self.interposer = nullptr;
    }
    self.get_optimal_settings = nullptr;
    self.set_options = nullptr;
    self.options_applied[0] = self.options_applied[1] = false;
    self.initialised = false;
    say("streamline shut down");
    return RSF_DLSS_OK;
}

#else // RSF_HAVE_STREAMLINE
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
