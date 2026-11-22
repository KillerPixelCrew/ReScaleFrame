// Streamline's Vulkan entry points must come from sl.interposer.dll.  Keep its
// C++ ABI and typed frame token access in this file.
// The Rust lifecycle owner (runtime/vulkan/streamline/lifecycle) admits, loads,
// initializes, shuts down and unloads the interposer and tracks the legacy FG
// configuration and current token. This file keeps the exports, typed SDK calls and file check.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <cstdint>
#include <atomic>
#include <cstring>
#include "sdk_records_checks.h"
#include "wsi_abi.h"
#include "sdk_fg_options_checks.h"

#if defined(RSF_STREAMLINE_VULKAN_STATIC)
#define RSF_SL_VK_EXPORT
#else
#define RSF_SL_VK_EXPORT __declspec(dllexport)
#endif

// Native leaves called by the Rust lifecycle owner, in its Platform order.
struct RsfSlLifecyclePlatform {
    void* (*openVerified)(const wchar_t* path);
    int32_t (*resolve)(void* module);
    int32_t (*init)(const rsf_slvk_sdk_preferences*, uint64_t);
    uint32_t (*shutdown)(int32_t* result);
    void (*clear)();
    rsf_slvk_sdk_log log;
};
static_assert(sizeof(RsfSlLifecyclePlatform) == 6 * sizeof(void*));

struct RsfSlFgOptions {
    uint32_t count, width, height, format, backBuffers;
};
static_assert(sizeof(RsfSlFgOptions) == 5 * sizeof(uint32_t));
struct RsfSlFgPlatform {
    uint32_t (*optionsBound)();
    int32_t (*apply)(const rsf_slvk_sdk_fg_options*);
    int32_t (*release)();
};
static_assert(sizeof(RsfSlFgPlatform) == 3 * sizeof(void*));
static_assert(static_cast<int32_t>(sl::Result::eOk) == 0);
static_assert(uint32_t(sl::LogType::eInfo) == 0 && uint32_t(sl::LogType::eWarn) == 1 && uint32_t(sl::LogType::eError) == 2);
static_assert(static_cast<int32_t>(sl::Result::eWarnOutOfVRAM) == 39);
static_assert(static_cast<uint32_t>(sl::ReflexMode::eOff) == 0 &&
    static_cast<uint32_t>(sl::ReflexMode::eLowLatency) == 1 &&
    static_cast<uint32_t>(sl::ReflexMode::eLowLatencyWithBoost) == 2);

extern "C" {
HANDLE rsf_slvk_open_verified(const wchar_t*, const uint8_t*, uint32_t require_signature);
int32_t rsf_slvk_life_initialize(const RsfSlLifecyclePlatform*, const wchar_t*, const char*,
    uint32_t, uint32_t, uint32_t);
int32_t rsf_slvk_life_shutdown(const RsfSlLifecyclePlatform*);
int32_t rsf_slvk_life_unload(const RsfSlLifecyclePlatform*);
void* rsf_slvk_life_module();
void rsf_slvk_diagnostic_present_error(int32_t result);
int32_t rsf_slvk_diagnostic_take_present_error();
uint64_t rsf_slvk_diagnostic_worker_timeout_epoch();
void rsf_slvk_log_message(uint32_t kind, const char* message);
int32_t rsf_slvk_fg_set(const RsfSlFgPlatform*, int32_t, const RsfSlFgOptions*);
int32_t rsf_slvk_fg_disable(const RsfSlFgPlatform*);
int32_t rsf_slvk_fg_free(const RsfSlFgPlatform*);
int32_t rsf_slvk_fg_state(int32_t (*)(rsf_slvk_sdk_fg_sample*), uint32_t*, uint32_t*, uint32_t*);
int32_t rsf_slvk_fg_state_details(int32_t (*)(rsf_slvk_sdk_fg_sample*), uint32_t*, uint32_t);
int32_t rsf_slvk_sdk_call(int32_t (*)(void*), void*, int32_t inactive);
int32_t rsf_slvk_reflex_options(int32_t (*)(const rsf_slvk_sdk_reflex_options*), int32_t mode, uint32_t limit, uint32_t limitIsFps);
int32_t rsf_slvk_token_begin(int32_t (*)(uint32_t, uintptr_t*), uint32_t, uintptr_t*);
uintptr_t rsf_slvk_token_current();
int32_t rsf_slvk_token_call(uintptr_t, int32_t (*)(uintptr_t, void*), void*, int32_t inactive);
}

namespace {
PFun_slInit* init{};
PFun_slShutdown* shutdown{};
PFun_slGetFeatureFunction* featureFunction{};
PFun_slGetNewFrameToken* newFrameToken{};
PFun_slIsFeatureSupported* isFeatureSupported{};
PFun_slSetTagForFrame* setTagForFrame{};
PFun_slSetConstants* setConstants{};
PFun_slFreeResources* freeResources{};
PFun_slReflexSetOptions* reflexOptions{};
std::atomic<PFun_slReflexSleep*> reflexSleep{};
PFun_slReflexGetState* reflexState{};
std::atomic<PFun_slPCLSetMarker*> pclMarker{};
PFun_slPCLGetState* pclState{};
PFun_slPCLSetOptions* pclOptions{};
PFun_slDLSSGSetOptions* fgOptions{};
PFun_slDLSSGGetState* fgState{};
const sl::ViewportHandle viewport(0);

/// @brief Resolves a function from the supplied interposer without acquiring another module reference.
template<class T> T entry(HMODULE library, const char* name) { return reinterpret_cast<T>(GetProcAddress(library, name)); }
/// @brief Clears the SDK entry points that slShutdown, a failed slInit or unload invalidates.
/// @details The Vulkan dispatch entry points are owned by the Rust lifecycle and stay until the module is unloaded.
void clearSdkFunctions() noexcept {
    init = nullptr; shutdown = nullptr; featureFunction = nullptr; newFrameToken = nullptr;
    isFeatureSupported = nullptr; setTagForFrame = nullptr; setConstants = nullptr; freeResources = nullptr;
    reflexOptions = nullptr; reflexSleep = nullptr; reflexState = nullptr; pclMarker = nullptr;
    pclState = nullptr; pclOptions = nullptr;
    fgOptions = nullptr; fgState = nullptr;
}

/// @brief Resolves one feature function only when Streamline reports success and a nonnull address.
bool loadFeature(sl::Feature feature, const char* name, void*& target) {
    return featureFunction && featureFunction(feature, name, target) == sl::Result::eOk && target;
}

// Keep the digest injected by the corrected build receipt or the pinned retail
// digest at the native build boundary. Rust verifies and owns the file handle.
HANDLE openVerifiedRelease(const wchar_t* path) noexcept {
    constexpr uint8_t expected[32] = {
#ifdef VULKANSTORY_CORRECTED_STREAMLINE_SHA256
        VULKANSTORY_CORRECTED_STREAMLINE_SHA256
#else
        0x8c, 0x87, 0xc9, 0x49, 0x94, 0x61, 0xda, 0x56,
        0x1e, 0xdd, 0x52, 0x9a, 0xa9, 0xbf, 0x78, 0x31,
        0xd6, 0x7d, 0x7b, 0x94, 0xeb, 0xb1, 0xc1, 0xa5,
        0xed, 0x54, 0xef, 0x49, 0x34, 0xe1, 0xea, 0x4c,
#endif
    };
#ifdef VULKANSTORY_CORRECTED_STREAMLINE_SHA256
    return rsf_slvk_open_verified(path, expected, 0);
#else
    return rsf_slvk_open_verified(path, expected, 1);
#endif
}

// Called by Streamline's present thread. Keep it lock-free and let the render
// thread report or recover from the error on its next state query.
/// @brief Publishes the latest asynchronous Vulkan presentation error with an atomic store.
/// @details Called by Streamline presentation work; host reporting/recovery consumes the error separately.
void onPresentError(const sl::APIError& error) {
    rsf_slvk_diagnostic_present_error(static_cast<int32_t>(error.vkRes));
}

// Rust invokes these leaves only while holding the initialized SDK session.
uint32_t fgOptionsBound() noexcept { return fgOptions != nullptr; }

int32_t applyFgOptions(const rsf_slvk_sdk_fg_options* request) noexcept try {
    sl::DLSSGOptions options{};
    std::memcpy(static_cast<void*>(&options), request, sizeof(options));
    options.onErrorCallback = onPresentError;
    return static_cast<int32_t>(fgOptions(viewport, options));
} catch (...) {
    return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
}

int32_t releaseFgResources() noexcept try {
    if (!freeResources) return -1;
    return static_cast<int32_t>(freeResources(sl::kFeatureDLSS_G, viewport));
} catch (...) {
    return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
}

constexpr RsfSlFgPlatform fgPlatform{fgOptionsBound, applyFgOptions, releaseFgResources};

// The Rust call is synchronous. It pins the SDK session for the borrowed callback.
template<class Call> int32_t callSdk(Call call, int32_t inactive = -1) {
    return rsf_slvk_sdk_call([](void* user) noexcept -> int32_t {
        try {
            return (*static_cast<Call*>(user))();
        } catch (...) {
            return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
        }
    }, &call, inactive);
}

int32_t applyReflexOptions(const rsf_slvk_sdk_reflex_options* request) noexcept try {
    if (!reflexOptions) return -1;
    sl::ReflexOptions options{};
    std::memcpy(static_cast<void*>(&options), request, sizeof(options));
    return static_cast<int32_t>(reflexOptions(options));
} catch (...) {
    return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
}

int32_t acquireFrameToken(uint32_t index, uintptr_t* token) noexcept try {
    if (!newFrameToken) return -1;
    sl::FrameToken* frame{};
    const auto result = newFrameToken(frame, &index);
    *token = reinterpret_cast<uintptr_t>(frame);
    return static_cast<int32_t>(result);
} catch (...) {
    return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
}

// Rust snapshots the current token when explicitToken is zero, and pins the
// initialized SDK until this synchronous callback returns.
template<class Call> int32_t withFrameToken(uintptr_t explicitToken, Call call, int32_t inactive = -1) {
    return rsf_slvk_token_call(explicitToken, [](uintptr_t token, void* user) noexcept -> int32_t {
        try {
            return (*static_cast<Call*>(user))(*reinterpret_cast<sl::FrameToken*>(token));
        } catch (...) {
            return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
        }
    }, &call, inactive);
}

int32_t emitFrameMarker(uintptr_t token, uint32_t marker) {
    return withFrameToken(token, [marker](const sl::FrameToken& frame) {
        const auto call = pclMarker.load();
        return call ? static_cast<int32_t>(call(static_cast<sl::PCLMarker>(marker), frame)) : -1;
    });
}

/// @brief Resolves the typed SDK exports from the module admitted by the Rust lifecycle.
/// @return Zero when every required export resolves.
int32_t lifecycleResolve(void* handle) noexcept {
    const auto library = static_cast<HMODULE>(handle);
    init = entry<PFun_slInit*>(library, "slInit");
    shutdown = entry<PFun_slShutdown*>(library, "slShutdown");
    featureFunction = entry<PFun_slGetFeatureFunction*>(library, "slGetFeatureFunction");
    newFrameToken = entry<PFun_slGetNewFrameToken*>(library, "slGetNewFrameToken");
    isFeatureSupported = entry<PFun_slIsFeatureSupported*>(library, "slIsFeatureSupported");
    setTagForFrame = entry<PFun_slSetTagForFrame*>(library, "slSetTagForFrame");
    setConstants = entry<PFun_slSetConstants*>(library, "slSetConstants");
    freeResources = entry<PFun_slFreeResources*>(library, "slFreeResources");
    return init && shutdown && featureFunction && newFrameToken && isFeatureSupported && setTagForFrame &&
        setConstants && freeResources ? 0 : -1;
}

int32_t lifecycleInit(const rsf_slvk_sdk_preferences* request, uint64_t sdkVersion) noexcept try {
    if (!init) return -4;
    sl::Preferences preferences{};
    std::memcpy(static_cast<void*>(&preferences), request, sizeof(preferences));
    return static_cast<int32_t>(init(preferences, sdkVersion));
} catch (...) {
    return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
}

/// @brief Calls slShutdown when it was resolved.
/// @return Nonzero when the SDK was called; result receives its result or the exception-handler error.
uint32_t lifecycleShutdown(int32_t* result) noexcept {
    if (!shutdown) return 0;
    try {
        *result = static_cast<int32_t>(shutdown());
    } catch (...) {
        *result = static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
    }
    return 1;
}

constexpr RsfSlLifecyclePlatform lifecycle{
    openVerifiedRelease, lifecycleResolve, lifecycleInit, lifecycleShutdown, clearSdkFunctions, rsf_slvk_log_message,
};
}

extern "C" {
/// @brief Loads the verified interposer and initializes the selected PCL/Reflex/DLSS feature set.
/// @details A repeated request must match the active directory, project and feature set. After shutdown, a later call reinitializes the retained module from the same directory.
/// @param directory Borrowed terminated directory path containing the private interposer/plugins.
/// @param projectId Borrowed terminated project identity copied for SDK use.
/// @param loadReflex Requests Reflex plus PCL when DLSS-G is not requested.
/// @param loadDlssG Requests DLSS-G, Reflex and PCL.
/// @param loadDlssSr Adds DLSS SR to the shared session.
/// @return Zero on successful/already-initialized state, a negative local failure or native Streamline result.
RSF_SL_VK_EXPORT int rsf_vk_sl_initialize_features(const wchar_t* directory, const char* projectId,
    uint32_t loadReflex, uint32_t loadDlssG, uint32_t loadDlssSr) {
    return rsf_slvk_life_initialize(&lifecycle, directory, projectId, loadReflex, loadDlssG, loadDlssSr);
}
RSF_SL_VK_EXPORT int rsf_vk_sl_initialize(const wchar_t* directory, const char* projectId,
    uint32_t loadReflex, uint32_t loadDlssG) {
    return rsf_vk_sl_initialize_features(directory, projectId, loadReflex, loadDlssG, 0);
}
RSF_SL_VK_EXPORT void* rsf_vk_sl_module() { return rsf_slvk_life_module(); }
RSF_SL_VK_EXPORT uint64_t rsf_vk_sl_worker_timeout_epoch() { return rsf_slvk_diagnostic_worker_timeout_epoch(); }

/// @brief Shuts down Streamline and clears SDK function pointers/state while retaining the interposer module.
/// @details Call after provider work is retired. In-flight token calls return busy and allow retry. Vulkan dispatch pointers can still reference interposer code until Vulkan devices/instances are destroyed. The SDK shutdown result is sticky; a nonzero result refuses later initialization and unload. After success, a later rsf_vk_sl_initialize re-runs slInit.
RSF_SL_VK_EXPORT int rsf_vk_sl_shutdown_checked() {
    return rsf_slvk_life_shutdown(&lifecycle);
}

/// @brief Retains the original void ABI for hosts predating checked shutdown.
RSF_SL_VK_EXPORT void rsf_vk_sl_shutdown() {
    rsf_vk_sl_shutdown_checked();
}

// Vulkan dispatch pointers can still point into the interposer after slShutdown.
// Release its code only after the host has destroyed Vulkan and its API tables.
/// @brief Unloads interposer code and clears Vulkan dispatch pointers.
/// @details Call only after shutdown and after every Vulkan object/API table referencing this module is destroyed. Refused after a failed shutdown.
RSF_SL_VK_EXPORT int rsf_vk_sl_unload_checked() {
    return rsf_slvk_life_unload(&lifecycle);
}
RSF_SL_VK_EXPORT void rsf_vk_sl_unload() {
    rsf_vk_sl_unload_checked();
}

// Resolve feature functions after the Vulkan device exists and the SL proxy has seen it.
/// @brief Queries DLSS-G support for the selected borrowed physical device.
/// @details Feature functions are queried after the proxy has observed device creation.
/// @return Native Streamline support result, or -1 when unavailable.
RSF_SL_VK_EXPORT int rsf_vk_sl_is_frame_generation_supported(VkPhysicalDevice physical) {
    if (!physical) return -1;
    return callSdk([&] {
        if (!isFeatureSupported) return -1;
        sl::AdapterInfo adapter{};
        adapter.vkPhysicalDevice = physical;
        return static_cast<int>(isFeatureSupported(sl::kFeatureDLSS_G, adapter));
    });
}
/// @brief Resolves Reflex options, sleep and state entry points for the initialized device.
/// @return Zero when all required functions resolve, otherwise a negative local code.
RSF_SL_VK_EXPORT int rsf_vk_sl_bind_reflex() {
    return callSdk([] {
        void* function{};
        if (!loadFeature(sl::kFeatureReflex, "slReflexSetOptions", function)) return -1;
        reflexOptions = reinterpret_cast<PFun_slReflexSetOptions*>(function);
        if (!loadFeature(sl::kFeatureReflex, "slReflexSleep", function)) return -2;
        reflexSleep = reinterpret_cast<PFun_slReflexSleep*>(function);
        if (!loadFeature(sl::kFeatureReflex, "slReflexGetState", function)) return -3;
        reflexState = reinterpret_cast<PFun_slReflexGetState*>(function);
        return 0;
    });
}
/// @brief Resolves DLSS-G option and state entry points for the initialized device.
/// @return Zero on success, otherwise a negative local code.
RSF_SL_VK_EXPORT int rsf_vk_sl_bind_frame_generation() {
    return callSdk([] {
        void* function{};
        if (!loadFeature(sl::kFeatureDLSS_G, "slDLSSGSetOptions", function)) return -4;
        fgOptions = reinterpret_cast<PFun_slDLSSGSetOptions*>(function);
        if (!loadFeature(sl::kFeatureDLSS_G, "slDLSSGGetState", function)) return -5;
        fgState = reinterpret_cast<PFun_slDLSSGGetState*>(function);
        return 0;
    }, -4);
}
/// @brief Queries distinct low-latency and latency-report availability flags.
/// @details Outputs are written only after a successful SDK query.
/// @return Zero on success, -1 for unavailable functions/null outputs, or the SDK result.
RSF_SL_VK_EXPORT int rsf_vk_sl_get_reflex_state(uint32_t* available, uint32_t* reports) {
    if (!available || !reports) return -1;
    return callSdk([&] {
        if (!reflexState) return -1;
        sl::ReflexState state{};
        const auto result = reflexState(state);
        if (result != sl::Result::eOk) return static_cast<int>(result);
        *available = state.lowLatencyAvailable ? 1u : 0u;
        *reports = state.latencyReportAvailable ? 1u : 0u;
        return 0;
    });
}
/// @brief Resolves PCL entry points and assigns the current host thread as the SDK marker thread.
/// @return Zero on success, a negative local bind failure or the SDK option result.
RSF_SL_VK_EXPORT int rsf_vk_sl_bind_pcl() {
    return callSdk([] {
        void* function{};
        if (!loadFeature(sl::kFeaturePCL, "slPCLSetMarker", function)) return -1;
        pclMarker = reinterpret_cast<PFun_slPCLSetMarker*>(function);
        if (!loadFeature(sl::kFeaturePCL, "slPCLGetState", function)) return -2;
        pclState = reinterpret_cast<PFun_slPCLGetState*>(function);
        if (!loadFeature(sl::kFeaturePCL, "slPCLSetOptions", function)) return -3;
        pclOptions = reinterpret_cast<PFun_slPCLSetOptions*>(function);
        rsf_slvk_sdk_pcl_options packed{};
        rsf_slvk_pack_pcl_options(GetCurrentThreadId(), &packed);
        sl::PCLOptions options{};
        std::memcpy(static_cast<void*>(&options), &packed, sizeof(options));
        return static_cast<int>(pclOptions(options));
    });
}
/// @brief Queries the PCL statistics latency-ping window message.
/// @return Message ID on success, otherwise zero.
RSF_SL_VK_EXPORT uint32_t rsf_vk_sl_pcl_window_message() {
    uint32_t message = 0;
    const auto result = callSdk([&] {
        if (!pclState) return -1;
        sl::PCLState state{};
        const auto code = pclState(state);
        if (code == sl::Result::eOk) message = state.statsWindowMessage;
        return static_cast<int>(code);
    });
    return result == 0 ? message : 0;
}
/// @brief Applies Off/On/Boost mode and a minimum interval in microseconds.
/// @details The caller selects a single pacing authority; this bridge does not arbitrate competing provider limiters.
/// @param mode 0 selects Off, 1 On, 2 Boost; other values select Off.
/// @param intervalUs Minimum interval in microseconds, or zero for no cap.
/// @return SDK result, or -1 without the option function.
RSF_SL_VK_EXPORT int rsf_vk_sl_set_reflex_interval(int mode, uint32_t intervalUs) {
    return rsf_slvk_reflex_options(applyReflexOptions, mode, intervalUs, 0);
}
RSF_SL_VK_EXPORT int rsf_vk_sl_set_reflex(int mode, uint32_t maxFps) {
    return rsf_slvk_reflex_options(applyReflexOptions, mode, maxFps, 1);
}
/// @brief Obtains the current Streamline frame token for the supplied 32-bit renderer frame identity.
/// @details Successful acquisition replaces the Rust lifecycle's borrowed pointer.
/// @return SDK result, -1 without token acquisition or -7 for an overlapping acquisition.
RSF_SL_VK_EXPORT int rsf_vk_sl_begin_frame_checked(uint32_t frameIndex, uintptr_t* token) {
    if (!token) return -1;
    return rsf_slvk_token_begin(acquireFrameToken, frameIndex, token);
}
RSF_SL_VK_EXPORT int rsf_vk_sl_begin_frame(uint32_t frameIndex) {
    uintptr_t token = 0;
    return rsf_vk_sl_begin_frame_checked(frameIndex, &token);
}
/// @brief Returns the borrowed pointer to the most recently acquired SDK frame token.
/// @details The caller must not free the token and must retain it only within the SDK frame lifetime.
/// @return Token address, or zero before acquisition/after shutdown.
RSF_SL_VK_EXPORT uintptr_t rsf_vk_sl_current_frame_token() {
    return rsf_slvk_token_current();
}
RSF_SL_VK_EXPORT int rsf_vk_sl_token_matches(uintptr_t token, uint32_t index) {
    if (!token) return 0;
    return withFrameToken(token, [index](const sl::FrameToken& frame) {
        return static_cast<uint32_t>(frame) == index ? 1 : 0;
    }, 0) == 1;
}
/// @brief Runs Reflex sleep for the current borrowed frame token before host input polling.
/// @return SDK result, or -1 without sleep/token availability.
RSF_SL_VK_EXPORT int rsf_vk_sl_reflex_sleep() {
    return withFrameToken(0, [](const sl::FrameToken& frame) {
        const auto call = reflexSleep.load();
        return call ? static_cast<int32_t>(call(frame)) : -1;
    });
}
/// @brief Emits a PCL marker for the current borrowed frame token.
/// @return SDK result, or -1 when the marker/token is unavailable.
RSF_SL_VK_EXPORT int rsf_vk_sl_marker(uint32_t marker) {
    return emitFrameMarker(0, marker);
}
/// @brief Emits a PCL marker for an explicitly retained token during asynchronous presentation.
/// @details The token is borrowed and must remain within its SDK lifetime; this call does not replace current frame state.
/// @return SDK result, or -1 for unavailable markers/a zero token.
RSF_SL_VK_EXPORT int rsf_vk_sl_marker_for_token(uintptr_t token, uint32_t marker) {
    return token ? emitFrameMarker(token, marker) : -1;
}

static int submitTags(const sl::FrameToken& frame, VkCommandBuffer commands,
    const rsf_slvk_legacy_image* const* images) {
    rsf_slvk_sdk_tag_batch packed{};
    rsf_slvk_pack_legacy_tags(images, &packed);
    sl::Resource resources[4]{};
    sl::ResourceTag tags[4]{};
    std::memcpy(static_cast<void*>(tags), packed.tags, sizeof(tags));
    if (images) {
        std::memcpy(static_cast<void*>(resources), packed.resources, sizeof(resources));
        for (size_t i = 0; i < 4; ++i) tags[i].resource = &resources[i];
    }
    return static_cast<int>(setTagForFrame(frame, viewport, tags, 4,
        reinterpret_cast<sl::CommandBuffer*>(commands)));
}

static int tagFrame(const sl::FrameToken& frame, VkCommandBuffer commandBuffer,
    const rsf_slvk_legacy_image* depth, const rsf_slvk_legacy_image* motion, const rsf_slvk_legacy_image* hudless,
    const rsf_slvk_legacy_image* ui, const rsf_slvk_legacy_camera* camera) {
    if (!setTagForFrame || !setConstants || !depth || !motion || !hudless || !ui) return -1;
    rsf_slvk_sdk_constants packed{};
    if (!rsf_slvk_pack_legacy_camera(camera, &packed)) return -1;
    sl::Constants constants{};
    std::memcpy(static_cast<void*>(&constants), &packed, sizeof(constants));
    auto result = setConstants(constants, frame, viewport);
    if (result != sl::Result::eOk) return static_cast<int>(result);

    const rsf_slvk_legacy_image* images[] = {depth, motion, hudless, ui};
    return submitTags(frame, commandBuffer, images);
}
/// @brief Sets matching camera constants and tags depth, motion, HUD-free color and UI on the current token.
/// @details Input resources use eOnlyValidNow so the SDK records copies on commandBuffer before renderer staging images are reused. Full-window rendering leaves backbuffer size to the SDK, which resolves it after any deferred swapchain recreation; client backbuffer tags are needed only for subregions.
/// @param commandBuffer Borrowed recording Vulkan command buffer used for immediate resource copies.
/// @param depth Borrowed depth metadata.
/// @param motion Borrowed motion-vector metadata.
/// @param hudless Borrowed HUD-free color metadata.
/// @param ui Borrowed premultiplied UI metadata.
/// @param camera Borrowed matching constants/matrix pointers.
/// @return SDK result, or -1 for missing required bindings/pointers.
RSF_SL_VK_EXPORT int rsf_vk_sl_tag_frame(VkCommandBuffer commandBuffer,
    const rsf_slvk_legacy_image* depth, const rsf_slvk_legacy_image* motion, const rsf_slvk_legacy_image* hudless,
    const rsf_slvk_legacy_image* ui, const rsf_slvk_legacy_camera* camera) {
    return withFrameToken(0, [&](const sl::FrameToken& frame) {
        return tagFrame(frame, commandBuffer, depth, motion, hudless, ui, camera);
    });
}
/// @brief Clears scene resource tags for the current frame token.
/// @details Use at non-scene frames or before resources/provider state change. The SDK resolves the full backbuffer after deferred swapchain recreation; no client subregion is supplied.
/// @return SDK result, or -1 without tagging/token availability.
RSF_SL_VK_EXPORT int rsf_vk_sl_invalidate_frame_tags() {
    return withFrameToken(0, [](const sl::FrameToken& frame) {
        return setTagForFrame ? submitTags(frame, nullptr, nullptr) : -1;
    });
}
/// @brief Applies DLSS-G enablement, interpolation count and presentation-resource metadata.
/// @details Successful identical options are reused. An Off out-of-VRAM warning clears configuration without proving release; the host must drain and explicitly free resources.
/// @return SDK result, -1 without options or -2 for an enabled zero interpolation count.
RSF_SL_VK_EXPORT int rsf_vk_sl_set_frame_generation(int enabled, uint32_t count,
    uint32_t width, uint32_t height, uint32_t colorFormat, uint32_t backBuffers) {
    const RsfSlFgOptions request{count, width, height, colorFormat, backBuffers};
    return rsf_slvk_fg_set(&fgPlatform, enabled, &request);
}
// Called after FG is off and the proxy's DeviceWaitIdle has drained presentation.
// Direct NGX SR shutdown must not precede release of Streamline's FG handle.
/// @brief Disables a previously enabled DLSS-G feature before the host's checked drain/release sequence.
/// @details Unused or already disabled features return zero without configuring another provider's presentation.
/// @return SDK option result, or zero when no disable is required.
RSF_SL_VK_EXPORT int rsf_vk_sl_disable_frame_generation_for_release() {
    return rsf_slvk_fg_disable(&fgPlatform);
}
/// @brief Frees known DLSS-G resources after enablement is Off and the host has drained proxy GPU/present work.
/// @details Resource-ownership state clears only when slFreeResources succeeds.
/// @return SDK result, zero for no resources, or -1 when release prerequisites are missing.
RSF_SL_VK_EXPORT int rsf_vk_sl_free_frame_generation_resources() {
    return rsf_slvk_fg_free(&fgPlatform);
}
/// @brief Atomically consumes the latest recorded asynchronous presentation error.
/// @return Recorded Vulkan error code, or zero when none is pending.
RSF_SL_VK_EXPORT int rsf_vk_sl_take_present_error() {
    return rsf_slvk_diagnostic_take_present_error();
}
static int32_t readFgSample(rsf_slvk_sdk_fg_sample* output) noexcept try {
    if (!fgState) return -1;
    sl::DLSSGState state{};
    const auto result = fgState(viewport, state, nullptr);
    if (result == sl::Result::eOk) {
        *output = {static_cast<uint32_t>(state.status), state.numFramesActuallyPresented,
            state.numFramesToGenerateMax, state.minWidthOrHeight,
            static_cast<uint8_t>(state.bIsVsyncSupportAvailable), static_cast<uint8_t>(state.bIsDynamicMFGSupported)};
    }
    return static_cast<int32_t>(result);
} catch (...) {
    return static_cast<int32_t>(sl::Result::eErrorExceptionHandler);
}

/// @brief Queries frame-generation status, actual presents and the generated-frame limit.
/// @details Output values are written only after a successful SDK state query.
/// @return SDK result, or -1 for unavailable state/null outputs.
RSF_SL_VK_EXPORT int rsf_vk_sl_get_frame_generation_state(uint32_t* status,
    uint32_t* presented, uint32_t* maxGenerated) {
    return rsf_slvk_fg_state(readFgSample, status, presented, maxGenerated);
}
/// @brief Queries the six-word frame-generation capability/state ABI.
/// @details Words contain status, actual presents, maximum generated count, minimum size, VSync availability and dynamic-MFG support in that order.
/// @param values Caller-owned output array containing exactly six uint32_t values.
/// @param count Must equal six.
/// @return SDK result, or -1 for unavailable state/invalid output storage.
RSF_SL_VK_EXPORT int rsf_vk_sl_get_frame_generation_state_details(uint32_t* values, uint32_t count) {
    return rsf_slvk_fg_state_details(readFgSample, values, count);
}
}
