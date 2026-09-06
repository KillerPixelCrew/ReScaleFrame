// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/dlss.h>

#if RSF_HAVE_STREAMLINE

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>

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

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <string>

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

    ID3D11Device* device = nullptr;
    bool initialised = false;
    bool supported = false;

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

void say(const char* format, ...)
{
    State& self = state();
    if (!self.log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    self.log(self.log_user, message);
}

// Streamline's own log messages, forwarded to the same sink rather than to a console nobody sees.
void streamline_message(sl::LogType type, const char* message)
{
    const char* label = type == sl::LogType::eError     ? "error"
                        : type == sl::LogType::eWarn    ? "warning"
                                                        : "info";
    say("streamline %s: %s", label, message ? message : "");
}

bool widen(const char* utf8, std::wstring& out)
{
    if (!utf8 || !*utf8) {
        out.clear();
        return false;
    }
    const int needed = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, nullptr, 0);
    if (needed <= 0) {
        out.clear();
        return false;
    }
    out.resize(static_cast<size_t>(needed) - 1);
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, out.data(), needed);
    return true;
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

sl::float4x4 to_matrix(const float source[16])
{
    // Row major on both sides, so this is a copy rather than a transpose. Streamline states row
    // major in sl_consts.h and Unreal's matrices are row major too, which is the one piece of luck
    // in this conversion.
    sl::float4x4 matrix{};
    for (int row = 0; row < 4; ++row) {
        matrix.row[row].x = source[row * 4 + 0];
        matrix.row[row].y = source[row * 4 + 1];
        matrix.row[row].z = source[row * 4 + 2];
        matrix.row[row].w = source[row * 4 + 3];
    }
    return matrix;
}

sl::Boolean flag(uint32_t value)
{
    return value ? sl::Boolean::eTrue : sl::Boolean::eFalse;
}

sl::DLSSMode mode_for(rsf_dlss_quality quality)
{
    switch (quality) {
    case RSF_DLSS_QUALITY_NATIVE:
        return sl::DLSSMode::eDLAA;
    case RSF_DLSS_QUALITY_QUALITY:
        return sl::DLSSMode::eMaxQuality;
    case RSF_DLSS_QUALITY_BALANCED:
        return sl::DLSSMode::eBalanced;
    case RSF_DLSS_QUALITY_PERFORMANCE:
        return sl::DLSSMode::eMaxPerformance;
    case RSF_DLSS_QUALITY_ULTRA_PERFORMANCE:
        return sl::DLSSMode::eUltraPerformance;
    default:
        return sl::DLSSMode::eOff;
    }
}

// The one viewport this integration drives. A second one would need its own tags, constants and
// options, so it gets a name rather than a bare zero to make that assumption visible.
const sl::ViewportHandle& sole_viewport()
{
    static sl::ViewportHandle handle{0u};
    return handle;
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
    self.log = setup->log;
    self.log_user = setup->log_user;

    std::wstring interposer;
    if (!widen(setup->interposer_path_utf8, interposer)) {
        return RSF_DLSS_ERROR_INVALID_ARGUMENT;
    }

    // This loads a signed NVIDIA module into a game process from a configured path. Verifying the
    // signature is the difference between that and loading whatever is at that path.
    if (setup->require_signature) {
#if RSF_HAVE_SIGNATURE_CHECK
        if (!sl::security::verifyEmbeddedSignature(interposer.c_str())) {
            say("interposer at %s failed signature verification", setup->interposer_path_utf8);
            return RSF_DLSS_ERROR_LOAD_FAILED;
        }
#else
        say("signature verification was asked for but is not compiled into this build");
        return RSF_DLSS_ERROR_LOAD_FAILED;
#endif
    }

    // By absolute path, never by name: the search path must not get to decide which module answers
    // for the SDK inside somebody else's process.
    self.interposer = LoadLibraryW(interposer.c_str());
    if (!self.interposer) {
        say("could not load %s (error %lu)", setup->interposer_path_utf8, GetLastError());
        return RSF_DLSS_ERROR_LOAD_FAILED;
    }

    Entries& sl_entries = self.sl;
    const bool resolved = resolve(self.interposer, "slInit", sl_entries.init) &&
                          resolve(self.interposer, "slShutdown", sl_entries.shutdown) &&
                          resolve(self.interposer, "slIsFeatureSupported",
                                  sl_entries.is_feature_supported) &&
                          resolve(self.interposer, "slSetD3DDevice", sl_entries.set_device) &&
                          resolve(self.interposer, "slGetFeatureFunction",
                                  sl_entries.get_feature_function) &&
                          resolve(self.interposer, "slGetFeatureRequirements",
                                  sl_entries.get_feature_requirements) &&
                          resolve(self.interposer, "slGetNewFrameToken",
                                  sl_entries.get_new_frame_token) &&
                          resolve(self.interposer, "slSetConstants", sl_entries.set_constants) &&
                          resolve(self.interposer, "slSetTagForFrame",
                                  sl_entries.set_tag_for_frame) &&
                          resolve(self.interposer, "slEvaluateFeature",
                                  sl_entries.evaluate_feature) &&
                          resolve(self.interposer, "slAllocateResources",
                                  sl_entries.allocate_resources) &&
                          resolve(self.interposer, "slFreeResources", sl_entries.free_resources);
    if (!resolved) {
        FreeLibrary(self.interposer);
        self.interposer = nullptr;
        return RSF_DLSS_ERROR_MISSING_ENTRY_POINT;
    }

    sl::Preferences preferences{};
    // Manual hooking is what makes this integration possible at all. The regular mode expects to
    // be in place before the swap chain exists; we attach to a game that is already rendering, and
    // in manual hooking the D3D device may be created before slInit.
    preferences.flags |= sl::PreferenceFlags::eUseManualHooking;
    preferences.renderAPI = sl::RenderAPI::eD3D11;
    preferences.logLevel = sl::LogLevel::eDefault;
    preferences.showConsole = false;
    preferences.logMessageCallback = streamline_message;

    static const sl::Feature features[] = {sl::kFeatureDLSS};
    preferences.featuresToLoad = features;
    preferences.numFeaturesToLoad = 1;

    if (widen(setup->plugin_directory_utf8, self.plugin_directory)) {
        self.plugin_paths[0] = self.plugin_directory.c_str();
        preferences.pathsToPlugins = self.plugin_paths;
        preferences.numPathsToPlugins = 1;
    }
    if (widen(setup->log_directory_utf8, self.log_directory)) {
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
    if (!self.initialised || !self.device) {
        return RSF_DLSS_ERROR_NOT_READY;
    }

    // The adapter comes from the game's own device. Enumerating adapters instead would answer for
    // a GPU the game is not rendering on, which on a laptop is the usual case rather than a rare
    // one.
    IDXGIDevice* dxgi_device = nullptr;
    if (FAILED(self.device->QueryInterface(__uuidof(IDXGIDevice),
                                           reinterpret_cast<void**>(&dxgi_device))) ||
        !dxgi_device) {
        return RSF_DLSS_ERROR_NOT_READY;
    }
    IDXGIAdapter* adapter = nullptr;
    const HRESULT got_adapter = dxgi_device->GetAdapter(&adapter);
    dxgi_device->Release();
    if (FAILED(got_adapter) || !adapter) {
        return RSF_DLSS_ERROR_NOT_READY;
    }
    DXGI_ADAPTER_DESC description{};
    const HRESULT described = adapter->GetDesc(&description);
    adapter->Release();
    if (FAILED(described)) {
        return RSF_DLSS_ERROR_NOT_READY;
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

    self.supported = support->supported != 0u;
    say("DLSS support on this adapter: %s (streamline result %u, driver %u.%u, requires %u.%u)",
        self.supported ? "yes" : "no", static_cast<unsigned>(result),
        support->detected_driver_major, support->detected_driver_minor,
        support->required_driver_major, support->required_driver_minor);
    return self.supported ? RSF_DLSS_OK : RSF_DLSS_ERROR_NOT_SUPPORTED;
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
    options.mode = mode_for(plan->quality);
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

extern "C" rsf_dlss_result rsf_dlss_evaluate(void* d3d11_context, const rsf_dlss_frame* frame)
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
    if (!self.initialised || !self.device || !self.set_options) {
        return RSF_DLSS_ERROR_NOT_READY;
    }

    auto* command_buffer = static_cast<sl::CommandBuffer*>(d3d11_context);

    sl::FrameToken* token = nullptr;
    if (self.sl.get_new_frame_token(token, nullptr) != sl::Result::eOk || !token) {
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    sl::Constants constants{};
    constants.cameraViewToClip = to_matrix(frame->camera_view_to_clip);
    constants.clipToCameraView = to_matrix(frame->clip_to_camera_view);
    constants.clipToPrevClip = to_matrix(frame->clip_to_prev_clip);
    constants.prevClipToClip = to_matrix(frame->prev_clip_to_clip);
    constants.jitterOffset = {frame->jitter_x, frame->jitter_y};
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

    if (self.sl.set_constants(constants, *token, sole_viewport()) != sl::Result::eOk) {
        say("slSetConstants failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    sl::DLSSOptions options{};
    options.mode = mode_for(frame->quality);
    options.outputWidth = frame->output_width;
    options.outputHeight = frame->output_height;
    options.colorBuffersHDR = sl::Boolean::eTrue;
    // Auto exposure only when the game does not hand us its own. AC7 keeps one in a 1x1 target and
    // it is bound at the same pass as everything else here, so normally it does.
    options.useAutoExposure = frame->exposure ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    if (self.set_options(sole_viewport(), options) != sl::Result::eOk) {
        say("slDLSSSetOptions failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    const sl::Extent render_extent{0, 0, frame->render_width, frame->render_height};
    const sl::Extent output_extent{0, 0, frame->output_width, frame->output_height};
    const sl::Extent exposure_extent{0, 0, 1, 1};

    sl::Resource color_in{sl::ResourceType::eTex2d, frame->color_in, nullptr, nullptr, 0};
    sl::Resource color_out{sl::ResourceType::eTex2d, frame->color_out, nullptr, nullptr, 0};
    sl::Resource depth{sl::ResourceType::eTex2d, frame->depth, nullptr, nullptr, 0};
    sl::Resource motion{sl::ResourceType::eTex2d, frame->motion, nullptr, nullptr, 0};
    sl::Resource exposure{sl::ResourceType::eTex2d, frame->exposure, nullptr, nullptr, 0};

    // Everything is tagged as valid only now. These are engine scene targets that the game reuses
    // later in the same frame, and claiming otherwise would hand DLSS a buffer holding something
    // else by the time it reads it.
    sl::ResourceTag tags[] = {
        sl::ResourceTag{&color_in, sl::kBufferTypeScalingInputColor,
                        sl::ResourceLifecycle::eOnlyValidNow, &render_extent},
        sl::ResourceTag{&color_out, sl::kBufferTypeScalingOutputColor,
                        sl::ResourceLifecycle::eOnlyValidNow, &output_extent},
        sl::ResourceTag{&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow,
                        &render_extent},
        sl::ResourceTag{&motion, sl::kBufferTypeMotionVectors,
                        sl::ResourceLifecycle::eOnlyValidNow, &render_extent},
        sl::ResourceTag{&exposure, sl::kBufferTypeExposure, sl::ResourceLifecycle::eOnlyValidNow,
                        &exposure_extent},
    };
    const uint32_t tag_count = frame->exposure ? 5u : 4u;

    if (self.sl.set_tag_for_frame(*token, sole_viewport(), tags, tag_count, command_buffer) !=
        sl::Result::eOk) {
        say("slSetTagForFrame failed");
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }

    const sl::BaseStructure* inputs[] = {&sole_viewport()};
    const sl::Result result =
        self.sl.evaluate_feature(sl::kFeatureDLSS, *token, inputs, 1, command_buffer);
    if (result != sl::Result::eOk) {
        say("slEvaluateFeature failed with result %u", static_cast<unsigned>(result));
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_release_resources(void)
{
    State& self = state();
    if (!self.initialised) {
        return RSF_DLSS_ERROR_NOT_READY;
    }
    if (self.sl.free_resources(sl::kFeatureDLSS, sole_viewport()) != sl::Result::eOk) {
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    }
    return RSF_DLSS_OK;
}

extern "C" rsf_dlss_result rsf_dlss_shutdown(void)
{
    State& self = state();
    if (!self.initialised) {
        return RSF_DLSS_ERROR_NOT_READY;
    }
    self.sl.shutdown();
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
    self.initialised = false;
    self.supported = false;
    say("streamline shut down");
    return RSF_DLSS_OK;
}

#else // RSF_HAVE_STREAMLINE

// Built without the SDK. The contract still exists so callers compile and can say honestly that
// this build has no DLSS in it, rather than reporting a runtime failure that never happened.

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
