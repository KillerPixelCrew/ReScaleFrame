// Streamline's Vulkan entry points must come from sl.interposer.dll.  Keep its
// C++ ABI and frame tokens on this side of the managed/native boundary.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <wintrust.h>
#include <softpub.h>
#include <wincrypt.h>
#include <string>
#include <cstdint>
#include <atomic>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <vulkan/vulkan.h>
#include <sl.h>
#include <sl_dlss_g.h>
#include <sl_reflex.h>
#include <sl_pcl.h>

#if defined(RSF_STREAMLINE_VULKAN_STATIC)
#define RSF_SL_VK_EXPORT
#else
#define RSF_SL_VK_EXPORT __declspec(dllexport)
#endif

namespace {
HMODULE module{};
PFun_slInit* init{};
PFun_slShutdown* shutdown{};
PFun_slGetFeatureFunction* featureFunction{};
PFun_slGetNewFrameToken* newFrameToken{};
PFun_slIsFeatureSupported* isFeatureSupported{};
PFun_slSetTagForFrame* setTagForFrame{};
PFun_slSetConstants* setConstants{};
PFun_slFreeResources* freeResources{};
PFN_vkGetInstanceProcAddr instanceProc{};
PFN_vkGetDeviceProcAddr deviceProc{};
PFun_slReflexSetOptions* reflexOptions{};
PFun_slReflexSleep* reflexSleep{};
PFun_slReflexGetState* reflexState{};
PFun_slPCLSetMarker* pclMarker{};
PFun_slPCLGetState* pclState{};
PFun_slPCLSetOptions* pclOptions{};
PFun_slDLSSGSetOptions* fgOptions{};
PFun_slDLSSGGetState* fgState{};
// True between a successful slInit and the matching slShutdown. The module can
// stay loaded after shutdown, so it does not mean the SDK is initialized.
bool initialized{};
bool shutdownAttempted{};
int shutdownResult{};
bool fgConfigured{};
bool fgResourcesMayExist{};
bool fgEnabled{};
VkSwapchainKHR currentSwapchain{};
uint32_t fgCount{}, fgWidth{}, fgHeight{}, fgFormat{}, fgBackBuffers{};
sl::FrameToken* frameToken{};
std::atomic<int> lastPresentError{0};
std::atomic<uint64_t> workerTimeoutEpoch{0};
bool verboseLog{};
const sl::ViewportHandle viewport(0);
std::wstring pluginDirectory;
std::string projectIdentity;
const wchar_t* pluginPath{};

/// @brief Resolves a function from the currently loaded interposer without acquiring another module reference.
template<class T> T proc(const char* name) { return reinterpret_cast<T>(GetProcAddress(module, name)); }
template<class T> T commonProc(const char* name) {
    std::wstring path = pluginDirectory + L"\\sl.common.dll";
    std::replace(path.begin(), path.end(), L'/', L'\\');
    HMODULE common = GetModuleHandleW(path.c_str());
    return common ? reinterpret_cast<T>(GetProcAddress(common, name)) : nullptr;
}

/// @brief Clears the SDK entry points and frame token that slShutdown or a failed slInit invalidates.
/// @details The Vulkan dispatch entry points stay: Vulkan objects may still use interposer code until the module is unloaded.
void clearSdkFunctions() {
    init = nullptr; shutdown = nullptr; featureFunction = nullptr; newFrameToken = nullptr;
    isFeatureSupported = nullptr; setTagForFrame = nullptr; setConstants = nullptr; freeResources = nullptr;
    reflexOptions = nullptr; reflexSleep = nullptr; reflexState = nullptr; pclMarker = nullptr;
    pclState = nullptr; pclOptions = nullptr;
    fgOptions = nullptr; fgState = nullptr; frameToken = nullptr;
}

/// @brief Resolves one feature function only when Streamline reports success and a nonnull address.
bool loadFeature(sl::Feature feature, const char* name, void*& target) {
    return featureFunction && featureFunction(feature, name, target) == sl::Result::eOk && target;
}

/// @brief Opens the interposer without write/delete sharing and verifies the build-pinned SHA-256 through that handle.
/// @details The returned handle keeps the verified file from being written, renamed or deleted; the caller holds it across LoadLibraryExW and closes it afterwards. WinTrust and cryptographic handles are released before returning.
/// @return The open verified file handle, or INVALID_HANDLE_VALUE when opening or verification fails.
HANDLE openVerifiedRelease(const wchar_t* path) {
    HANDLE input = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ,
        nullptr, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (input == INVALID_HANDLE_VALUE) return INVALID_HANDLE_VALUE;
    WINTRUST_FILE_INFO file{};
    file.cbStruct = sizeof(file);
    file.pcwszFilePath = path;
    file.hFile = input;
    WINTRUST_DATA trust{};
    trust.cbStruct = sizeof(trust);
    trust.dwUIChoice = WTD_UI_NONE;
    trust.fdwRevocationChecks = WTD_REVOKE_NONE;
    trust.dwUnionChoice = WTD_CHOICE_FILE;
    trust.pFile = &file;
    trust.dwStateAction = WTD_STATEACTION_VERIFY;
    trust.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL;
    GUID action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    const LONG signature = WinVerifyTrust(nullptr, &action, &trust);
    (void)signature;
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    WinVerifyTrust(nullptr, &action, &trust);
    LARGE_INTEGER start{};
    if (
#ifndef VULKANSTORY_CORRECTED_STREAMLINE_SHA256
        signature != ERROR_SUCCESS ||
#endif
        !SetFilePointerEx(input, start, nullptr, FILE_BEGIN)) {
        CloseHandle(input);
        return INVALID_HANDLE_VALUE;
    }

    // Source builds embed their own interposer hash. The original release path
    // additionally requires Authenticode. Neither accepts a different binary.
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
    HCRYPTPROV provider{};
    HCRYPTHASH hash{};
    uint8_t digest[32]{};
    DWORD size = sizeof(digest);
    bool valid = CryptAcquireContextW(&provider, nullptr, nullptr, PROV_RSA_AES, CRYPT_VERIFYCONTEXT) &&
        CryptCreateHash(provider, CALG_SHA_256, 0, 0, &hash);
    uint8_t bytes[64 * 1024];
    DWORD read{};
    while (valid) {
        if (!ReadFile(input, bytes, sizeof(bytes), &read, nullptr)) { valid = false; break; }
        if (read == 0) break;
        valid = CryptHashData(hash, bytes, read, 0) != FALSE;
    }
    if (valid) valid = CryptGetHashParam(hash, HP_HASHVAL, digest, &size, 0) &&
        size == sizeof(expected) && memcmp(digest, expected, size) == 0;
    if (hash) CryptDestroyHash(hash);
    if (provider) CryptReleaseContext(provider, 0);
    if (valid) return input;
    CloseHandle(input);
    return INVALID_HANDLE_VALUE;
}

/// @brief Borrowed Vulkan image/view metadata passed through the Streamline C ABI.
/// @details No ownership is acquired by this record. The associated tagged resource lifecycle determines when the original image can be reused.
struct TaggedImage {
    uint64_t image;
    uint64_t view;
    uint32_t layout;
    uint32_t format;
    uint32_t width;
    uint32_t height;
    uint32_t usage;
};

/// @brief Camera matrices, vectors, jitter and motion scales for matching Streamline frame tags.
/// @details Matrix pointers reference caller-owned column-major 4x4 floats valid during the tagging call; rowMajor converts them for Streamline.
struct FrameCamera {
    const float* viewToClip;
    const float* clipToView;
    const float* clipToPrevClip;
    const float* prevClipToClip;
    float nearPlane, farPlane, fov, aspect;
    float jitterX, jitterY, mvecScaleX, mvecScaleY;
    float position[3], up[3], right[3], forward[3];
    uint32_t reset;
};

/// @brief Builds a borrowed Streamline 2D resource description from one C-ABI image record.
sl::Resource resource(const TaggedImage& image) {
    sl::Resource value{sl::ResourceType::eTex2d,
        reinterpret_cast<void*>(static_cast<uintptr_t>(image.image)), nullptr,
        reinterpret_cast<void*>(static_cast<uintptr_t>(image.view)), image.layout};
    value.width = image.width; value.height = image.height;
    value.nativeFormat = image.format; value.mipLevels = 1; value.arrayLayers = 1;
    value.flags = 0;
    value.usage = image.usage;
    return value;
}

/// @brief Converts caller column-vector matrices to Streamline's row-vector convention.
void rowMajor(sl::float4x4& target, const float* columnMajor) {
    for (int row = 0; row < 4; ++row)
        for (int col = 0; col < 4; ++col)
            (&target[row].x)[col] = columnMajor[row * 4 + col];
}

// Called by Streamline's present thread. Keep it lock-free and let the render
// thread report or recover from the error on its next state query.
/// @brief Publishes the latest asynchronous Vulkan presentation error with an atomic store.
/// @details Called by Streamline presentation work; host reporting/recovery consumes the error separately.
void onPresentError(const sl::APIError& error) {
    lastPresentError.store(static_cast<int>(error.vkRes), std::memory_order_relaxed);
}

/// @brief Writes SDK warnings/errors and enabled verbose messages to stderr.
void onStreamlineMessage(sl::LogType type, const char* message) {
    if (message && std::strstr(message, "Worker thread '") && std::strstr(message, "' timed out"))
        workerTimeoutEpoch.fetch_add(1, std::memory_order_relaxed);
    if ((type == sl::LogType::eInfo && !verboseLog) || !message) return;
    std::fprintf(stderr, "[Streamline %s] %s\n",
        type == sl::LogType::eError ? "error" : type == sl::LogType::eWarn ? "warning" : "info", message);
}
}

extern "C" {
/// @brief Loads the verified interposer and initializes the selected PCL/Reflex/DLSS feature set.
/// @details Global bridge state is owned by one serialized host session. Plugin path and project identity are copied into persistent strings. Repeated initialization while initialized returns zero. After VulkanStorySlShutdown the interposer stays loaded, so a later call re-runs slInit on that module, which must come from the same directory.
/// @param directory Borrowed terminated directory path containing the private interposer/plugins.
/// @param projectId Borrowed terminated project identity copied for SDK use.
/// @param loadReflex Requests Reflex plus PCL when DLSS-G is not requested.
/// @param loadDlssG Requests DLSS-G, Reflex and PCL.
/// @param loadDlssSr Adds DLSS SR to the shared session.
/// @return Zero on successful/already-initialized state, a negative local failure or native Streamline result.
RSF_SL_VK_EXPORT int VulkanStorySlInitializeFeatures(const wchar_t* directory, const char* projectId,
    uint32_t loadReflex, uint32_t loadDlssG, uint32_t loadDlssSr) {
    if (shutdownAttempted && shutdownResult != 0) return shutdownResult;
    if (initialized) return 0;
    if (!directory || !projectId) return -1;
    // A module kept loaded after shutdown may still back Vulkan dispatch tables;
    // only a module loaded by this call is released on failure.
    const bool loadedHere = !module;
    if (loadedHere) {
        wchar_t path[MAX_PATH]{};
        if (swprintf(path, MAX_PATH, L"%ls\\sl.interposer.dll", directory) < 0) return -2;
        // The verified handle denies writes, renames and deletes until the loader
        // has mapped the same file.
        HANDLE verified = openVerifiedRelease(path);
        if (verified == INVALID_HANDLE_VALUE) return -5;
        module = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        CloseHandle(verified);
        if (!module) return -3;
    } else if (pluginDirectory != directory) {
        return -6;
    }
    init = proc<PFun_slInit*>("slInit");
    shutdown = proc<PFun_slShutdown*>("slShutdown");
    featureFunction = proc<PFun_slGetFeatureFunction*>("slGetFeatureFunction");
    newFrameToken = proc<PFun_slGetNewFrameToken*>("slGetNewFrameToken");
    isFeatureSupported = proc<PFun_slIsFeatureSupported*>("slIsFeatureSupported");
    setTagForFrame = proc<PFun_slSetTagForFrame*>("slSetTagForFrame");
    setConstants = proc<PFun_slSetConstants*>("slSetConstants");
    freeResources = proc<PFun_slFreeResources*>("slFreeResources");
    instanceProc = proc<PFN_vkGetInstanceProcAddr>("vkGetInstanceProcAddr");
    deviceProc = proc<PFN_vkGetDeviceProcAddr>("vkGetDeviceProcAddr");
    if (!init || !shutdown || !featureFunction || !newFrameToken || !isFeatureSupported || !setTagForFrame ||
        !setConstants || !freeResources || !instanceProc || !deviceProc) {
        clearSdkFunctions();
        if (loadedHere) { FreeLibrary(module); module = nullptr; }
        return -4;
    }
    pluginDirectory = directory;
    projectIdentity = projectId;
    pluginPath = pluginDirectory.c_str();
    sl::Feature features[4] = {sl::kFeaturePCL};
    uint32_t featureCount = 1;
    if (loadReflex || loadDlssG) features[featureCount++] = sl::kFeatureReflex;
    if (loadDlssG) features[featureCount++] = sl::kFeatureDLSS_G;
    if (loadDlssSr) features[featureCount++] = sl::kFeatureDLSS;
    sl::Preferences preferences{};
    preferences.featuresToLoad = features;
    preferences.numFeaturesToLoad = featureCount;
    preferences.projectId = projectIdentity.c_str();
    preferences.engine = sl::EngineType::eCustom;
    preferences.engineVersion = "0.3";
    preferences.renderAPI = sl::RenderAPI::eVulkan;
    verboseLog = GetEnvironmentVariableA("VULKANSTORY_STREAMLINE_VERBOSE", nullptr, 0) > 0;
    if (verboseLog) preferences.logLevel = sl::LogLevel::eVerbose;
    preferences.logMessageCallback = onStreamlineMessage;
    preferences.flags = sl::PreferenceFlags::eUseManualHooking |
        sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.pathsToPlugins = &pluginPath;
    preferences.numPathsToPlugins = 1;
    const auto result = init(preferences, sl::kSDKVersion);
    if (result != sl::Result::eOk) {
        clearSdkFunctions();
        if (loadedHere) { FreeLibrary(module); module = nullptr; }
        return static_cast<int>(result);
    }
    initialized = true;
    shutdownAttempted = false;
    shutdownResult = 0;
    return 0;
}
RSF_SL_VK_EXPORT int VulkanStorySlInitialize(const wchar_t* directory, const char* projectId,
    uint32_t loadReflex, uint32_t loadDlssG) {
    return VulkanStorySlInitializeFeatures(directory, projectId, loadReflex, loadDlssG, 0);
}
RSF_SL_VK_EXPORT void* VulkanStorySlModule() { return initialized ? module : nullptr; }
RSF_SL_VK_EXPORT uint64_t VulkanStorySlWorkerTimeoutEpoch() { return workerTimeoutEpoch.load(std::memory_order_relaxed); }

/// @brief Shuts down Streamline and clears SDK function pointers/state while retaining the interposer module.
/// @details Call after provider work is retired. Vulkan dispatch pointers can still reference interposer code until Vulkan devices/instances are destroyed. A later VulkanStorySlInitialize re-runs slInit.
RSF_SL_VK_EXPORT int VulkanStorySlShutdownChecked() {
    if (!module) return 0;
    if (shutdownAttempted) return shutdownResult;
    // An SDK failure may follow partial plugin destruction. Spend the attempt
    // before calling it; neither feature entry points nor retries remain valid.
    shutdownAttempted = true;
    if (initialized && shutdown) {
        shutdownResult = static_cast<int>(sl::Result::eErrorExceptionHandler);
        shutdownResult = static_cast<int>(shutdown());
    }
    initialized = false;
    // Only the Vulkan dispatch entry points remain valid after slShutdown.
    clearSdkFunctions();
    fgConfigured = false;
    fgResourcesMayExist = false;
    fgEnabled = false;
    currentSwapchain = VK_NULL_HANDLE;
    lastPresentError.store(0, std::memory_order_relaxed);
    return shutdownResult;
}

/// @brief Retains the original void ABI for hosts predating checked shutdown.
RSF_SL_VK_EXPORT void VulkanStorySlShutdown() {
    VulkanStorySlShutdownChecked();
}

// Vulkan dispatch pointers can still point into the interposer after slShutdown.
// Release its code only after the host has destroyed Vulkan and its API tables.
/// @brief Unloads interposer code and clears Vulkan dispatch pointers.
/// @details Call only after shutdown and after every Vulkan object/API table referencing this module is destroyed.
RSF_SL_VK_EXPORT void VulkanStorySlUnload() {
    if (shutdownAttempted && shutdownResult != 0) return;
    if (module) FreeLibrary(module);
    module = nullptr;
    initialized = false;
    instanceProc = nullptr;
    deviceProc = nullptr;
}

/// @brief Returns an interposer instance/global Vulkan function address.
/// @details Returned pointers borrow the loaded module lifetime.
/// @return Resolved address, or null when the module/lookup is unavailable.
RSF_SL_VK_EXPORT PFN_vkVoidFunction VulkanStorySlGetInstanceProcAddr(
    VkInstance instance, const char* name) {
    return module && instanceProc ? instanceProc(instance, name) : nullptr;
}
/// @brief Returns an interposer device Vulkan function address.
/// @details Returned pointers borrow the loaded module lifetime.
/// @return Resolved address, or null when the module/lookup is unavailable.
RSF_SL_VK_EXPORT PFN_vkVoidFunction VulkanStorySlGetDeviceProcAddr(
    VkDevice device, const char* name) {
    return module && deviceProc ? deviceProc(device, name) : nullptr;
}

/// @brief Forwards instance creation through interposer-owned Vulkan dispatch.
/// @return The Vulkan result, or VK_ERROR_INITIALIZATION_FAILED when dispatch is unavailable.
RSF_SL_VK_EXPORT VkResult VulkanStorySlCreateInstance(const VkInstanceCreateInfo* info, VkInstance* instance) {
    auto call = module ? reinterpret_cast<PFN_vkCreateInstance>(instanceProc(nullptr, "vkCreateInstance")) : nullptr;
    return call ? call(info, nullptr, instance) : VK_ERROR_INITIALIZATION_FAILED;
}
/// @brief Enumerates physical devices through the interposer using Vulkan count/query semantics.
/// @return The Vulkan enumeration result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlEnumeratePhysicalDevices(VkInstance instance,
    uint32_t* count, VkPhysicalDevice* devices) {
    auto call = module ? reinterpret_cast<PFN_vkEnumeratePhysicalDevices>(
        instanceProc(instance, "vkEnumeratePhysicalDevices")) : nullptr;
    return call ? call(instance, count, devices) : VK_ERROR_INITIALIZATION_FAILED;
}
/// @brief Creates a logical device through the interposer for the selected physical device.
/// @return The Vulkan creation result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlCreateDevice(VkInstance instance, VkPhysicalDevice physical,
    const VkDeviceCreateInfo* info, VkDevice* device) {
    auto call = module ? reinterpret_cast<PFN_vkCreateDevice>(instanceProc(instance, "vkCreateDevice")) : nullptr;
    return call ? call(physical, info, nullptr, device) : VK_ERROR_INITIALIZATION_FAILED;
}
/// @brief Creates a Win32 Vulkan surface through interposer dispatch.
/// @details The caller owns the returned surface and keeps its borrowed window valid through surface use.
/// @return The Vulkan creation result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlCreateWin32Surface(VkInstance instance,
    const VkWin32SurfaceCreateInfoKHR* info, VkSurfaceKHR* surface) {
    auto call = module ? reinterpret_cast<PFN_vkCreateWin32SurfaceKHR>(instanceProc(instance, "vkCreateWin32SurfaceKHR")) : nullptr;
    return call ? call(instance, info, nullptr, surface) : VK_ERROR_INITIALIZATION_FAILED;
}
/// @brief Destroys a caller-owned surface through its interposer dispatch when available.
/// @details The caller must first retire swapchains referring to this surface.
RSF_SL_VK_EXPORT void VulkanStorySlDestroySurface(VkInstance instance, VkSurfaceKHR surface) {
    auto call = module ? reinterpret_cast<PFN_vkDestroySurfaceKHR>(instanceProc(instance, "vkDestroySurfaceKHR")) : nullptr;
    if (call) call(instance, surface, nullptr);
}
/// @brief Creates an interposer swapchain and records it as the current presentation owner.
/// @details Successful creation invalidates cached dimensions/options without resetting feature enablement.
/// @return The Vulkan creation result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlCreateSwapchain(VkDevice device,
    const VkSwapchainCreateInfoKHR* info, VkSwapchainKHR* swapchain) {
    auto call = module ? reinterpret_cast<PFN_vkCreateSwapchainKHR>(deviceProc(device, "vkCreateSwapchainKHR")) : nullptr;
    VkResult result = call ? call(device, info, nullptr, swapchain) : VK_ERROR_INITIALIZATION_FAILED;
    if (result == VK_SUCCESS && swapchain) {
        currentSwapchain = *swapchain;
        fgConfigured = false;
    }
    return result;
}
/// @brief Destroys the supplied interposer swapchain after caller-provided retirement.
/// @details Destroying an old retired chain does not clear the live successor's cached state.
RSF_SL_VK_EXPORT void VulkanStorySlDestroySwapchain(VkDevice device, VkSwapchainKHR swapchain) {
    auto call = module ? reinterpret_cast<PFN_vkDestroySwapchainKHR>(deviceProc(device, "vkDestroySwapchainKHR")) : nullptr;
    if (!call) return;
    call(device, swapchain, nullptr);
    // Deferred retirement of an old chain must not invalidate the live
    // successor's options and cause a redundant SetOptions on its next frame.
    if (swapchain == currentSwapchain) {
        currentSwapchain = VK_NULL_HANDLE;
        fgConfigured = false;
    }
}
/// @brief Enumerates images belonging to the interposer-created swapchain.
/// @details Count and image storage use ordinary Vulkan query/enumeration rules.
/// @return The Vulkan enumeration result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlGetSwapchainImages(VkDevice device, VkSwapchainKHR swapchain,
    uint32_t* count, VkImage* images) {
    auto call = module ? reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(deviceProc(device, "vkGetSwapchainImagesKHR")) : nullptr;
    return call ? call(device, swapchain, count, images) : VK_ERROR_INITIALIZATION_FAILED;
}
/// @brief Acquires an image through the same interposer owner that created the chain.
/// @details Timeout and binary semaphore/fence pointers follow the borrowed Vulkan call contract.
/// @return The Vulkan acquisition result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlAcquire(VkDevice device, VkSwapchainKHR swapchain,
    uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t* index) {
    auto call = module ? reinterpret_cast<PFN_vkAcquireNextImageKHR>(deviceProc(device, "vkAcquireNextImageKHR")) : nullptr;
    return call ? call(device, swapchain, timeout, semaphore, fence, index) : VK_ERROR_INITIALIZATION_FAILED;
}
/// @brief Queues presentation through the interposer on the borrowed Vulkan queue.
/// @details Returned success does not establish GPU or display scanout completion.
/// @return The Vulkan queue-present result or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlPresent(VkDevice device, VkQueue queue, const VkPresentInfoKHR* info) {
    auto call = module ? reinterpret_cast<PFN_vkQueuePresentKHR>(deviceProc(device, "vkQueuePresentKHR")) : nullptr;
    return call ? call(queue, info) : VK_ERROR_INITIALIZATION_FAILED;
}
RSF_SL_VK_EXPORT int VulkanStorySlQueuePresentSynchronized(uintptr_t function, void* queue, const void* info) {
    auto present = reinterpret_cast<PFN_vkQueuePresentKHR>(function);
    if (!present || !queue || !info) return VK_ERROR_INITIALIZATION_FAILED;
    if (!initialized) return present(static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
    using Call = VkResult (*)(PFN_vkQueuePresentKHR, VkQueue, const VkPresentInfoKHR*);
    auto call = commonProc<Call>("rsfSlVulkanQueuePresent");
    return call ? call(present, static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info)) : VK_ERROR_EXTENSION_NOT_PRESENT;
}
RSF_SL_VK_EXPORT int VulkanStorySlVulkanWsiCall(int32_t (*callback)(void*), void* user) {
    if (!callback || !user) return VK_ERROR_INITIALIZATION_FAILED;
    if (!initialized) return callback(user);
    using Call = int32_t (*)(int32_t (*)(void*), void*);
    auto call = commonProc<Call>("rsfSlVulkanWsiCall");
    return call ? call(callback, user) : VK_ERROR_EXTENSION_NOT_PRESENT;
}
RSF_SL_VK_EXPORT int VulkanStorySlWithoutDlssG(int32_t (*callback)(void*), void* user) {
    if (!callback) return VK_ERROR_INITIALIZATION_FAILED;
    if (!initialized) return callback(user);
    using Call = int32_t (*)(int32_t (*)(void*), void*);
    auto call = reinterpret_cast<Call>(GetProcAddress(module, "rsfSlVulkanWithoutDlssGCall"));
    return call ? call(callback, user) : VK_ERROR_EXTENSION_NOT_PRESENT;
}
/// @brief Forwards logical-device idle waiting through interposer dispatch.
/// @return The checked Vulkan wait result, or initialization failure.
RSF_SL_VK_EXPORT VkResult VulkanStorySlDeviceWaitIdle(VkDevice device) {
    auto call = module ? reinterpret_cast<PFN_vkDeviceWaitIdle>(deviceProc(device, "vkDeviceWaitIdle")) : nullptr;
    return call ? call(device) : VK_ERROR_INITIALIZATION_FAILED;
}

// Resolve feature functions after the Vulkan device exists and the SL proxy has seen it.
/// @brief Queries DLSS-G support for the selected borrowed physical device.
/// @details Feature functions are queried after the proxy has observed device creation.
/// @return Native Streamline support result, or -1 when unavailable.
RSF_SL_VK_EXPORT int VulkanStorySlIsFrameGenerationSupported(VkPhysicalDevice physical) {
    if (!isFeatureSupported || !physical) return -1;
    sl::AdapterInfo adapter{};
    adapter.vkPhysicalDevice = physical;
    return static_cast<int>(isFeatureSupported(sl::kFeatureDLSS_G, adapter));
}
/// @brief Resolves Reflex options, sleep and state entry points for the initialized device.
/// @return Zero when all required functions resolve, otherwise a negative local code.
RSF_SL_VK_EXPORT int VulkanStorySlBindReflex() {
    void* function{};
    if (!loadFeature(sl::kFeatureReflex, "slReflexSetOptions", function)) return -1;
    reflexOptions = reinterpret_cast<PFun_slReflexSetOptions*>(function);
    if (!loadFeature(sl::kFeatureReflex, "slReflexSleep", function)) return -2;
    reflexSleep = reinterpret_cast<PFun_slReflexSleep*>(function);
    if (!loadFeature(sl::kFeatureReflex, "slReflexGetState", function)) return -3;
    reflexState = reinterpret_cast<PFun_slReflexGetState*>(function);
    return 0;
}
/// @brief Resolves DLSS-G option and state entry points for the initialized device.
/// @return Zero on success, otherwise a negative local code.
RSF_SL_VK_EXPORT int VulkanStorySlBindFrameGeneration() {
    void* function{};
    if (!loadFeature(sl::kFeatureDLSS_G, "slDLSSGSetOptions", function)) return -4;
    fgOptions = reinterpret_cast<PFun_slDLSSGSetOptions*>(function);
    if (!loadFeature(sl::kFeatureDLSS_G, "slDLSSGGetState", function)) return -5;
    fgState = reinterpret_cast<PFun_slDLSSGGetState*>(function);
    return 0;
}
/// @brief Queries distinct low-latency and latency-report availability flags.
/// @details Outputs are written only after a successful SDK query.
/// @return Zero on success, -1 for unavailable functions/null outputs, or the SDK result.
RSF_SL_VK_EXPORT int VulkanStorySlGetReflexState(uint32_t* available, uint32_t* reports) {
    if (!reflexState || !available || !reports) return -1;
    sl::ReflexState state{};
    auto result = reflexState(state);
    if (result != sl::Result::eOk) return static_cast<int>(result);
    *available = state.lowLatencyAvailable ? 1u : 0u;
    *reports = state.latencyReportAvailable ? 1u : 0u;
    return 0;
}
/// @brief Resolves PCL entry points and assigns the current host thread as the SDK marker thread.
/// @return Zero on success, a negative local bind failure or the SDK option result.
RSF_SL_VK_EXPORT int VulkanStorySlBindPcl() {
    void* function{};
    if (!loadFeature(sl::kFeaturePCL, "slPCLSetMarker", function)) return -1;
    pclMarker = reinterpret_cast<PFun_slPCLSetMarker*>(function);
    if (!loadFeature(sl::kFeaturePCL, "slPCLGetState", function)) return -2;
    pclState = reinterpret_cast<PFun_slPCLGetState*>(function);
    if (!loadFeature(sl::kFeaturePCL, "slPCLSetOptions", function)) return -3;
    pclOptions = reinterpret_cast<PFun_slPCLSetOptions*>(function);
    sl::PCLOptions options{};
    options.idThread = GetCurrentThreadId();
    return static_cast<int>(pclOptions(options));
}
/// @brief Queries the PCL statistics latency-ping window message.
/// @return Message ID on success, otherwise zero.
RSF_SL_VK_EXPORT uint32_t VulkanStorySlPclWindowMessage() {
    if (!pclState) return 0;
    sl::PCLState state{};
    return pclState(state) == sl::Result::eOk ? state.statsWindowMessage : 0;
}
/// @brief Applies Off/On/Boost mode and a minimum interval in microseconds.
/// @details The caller selects a single pacing authority; this bridge does not arbitrate competing provider limiters.
/// @param mode 0 selects Off, 1 On, 2 Boost; other values select Off.
/// @param intervalUs Minimum interval in microseconds, or zero for no cap.
/// @return SDK result, or -1 without the option function.
RSF_SL_VK_EXPORT int VulkanStorySlSetReflexInterval(int mode, uint32_t intervalUs) {
    if (!reflexOptions) return -1;
    sl::ReflexOptions options{};
    options.mode = mode == 2 ? sl::ReflexMode::eLowLatencyWithBoost :
        mode == 1 ? sl::ReflexMode::eLowLatency : sl::ReflexMode::eOff;
    options.frameLimitUs = intervalUs;
    options.idThread = GetCurrentThreadId();
    return static_cast<int>(reflexOptions(options));
}
RSF_SL_VK_EXPORT int VulkanStorySlSetReflex(int mode, uint32_t maxFps) {
    return VulkanStorySlSetReflexInterval(mode, maxFps ? 1000000u / maxFps : 0);
}
/// @brief Obtains the current Streamline frame token for the supplied 32-bit renderer frame identity.
/// @details The bridge stores the SDK-owned token pointer; new-frame acquisition replaces that borrowed pointer.
/// @return SDK result, or -1 without token acquisition.
RSF_SL_VK_EXPORT int VulkanStorySlBeginFrame(uint32_t frameIndex) {
    if (!newFrameToken) return -1;
    return static_cast<int>(newFrameToken(frameToken, &frameIndex));
}
/// @brief Returns the borrowed pointer to the most recently acquired SDK frame token.
/// @details The caller must not free the token and must retain it only within the SDK frame lifetime.
/// @return Token address, or zero before acquisition/after shutdown.
RSF_SL_VK_EXPORT uintptr_t VulkanStorySlCurrentFrameToken() {
    return reinterpret_cast<uintptr_t>(frameToken);
}
RSF_SL_VK_EXPORT int VulkanStorySlTokenMatches(uintptr_t token, uint32_t index) {
    return token && static_cast<uint32_t>(*reinterpret_cast<sl::FrameToken*>(token)) == index;
}
/// @brief Runs Reflex sleep for the current borrowed frame token before host input polling.
/// @return SDK result, or -1 without sleep/token availability.
RSF_SL_VK_EXPORT int VulkanStorySlReflexSleep() {
    return reflexSleep && frameToken ? static_cast<int>(reflexSleep(*frameToken)) : -1;
}
/// @brief Emits a PCL marker for the current borrowed frame token.
/// @return SDK result, or -1 when the marker/token is unavailable.
RSF_SL_VK_EXPORT int VulkanStorySlMarker(uint32_t marker) {
    return pclMarker && frameToken ? static_cast<int>(pclMarker(static_cast<sl::PCLMarker>(marker), *frameToken)) : -1;
}
/// @brief Emits a PCL marker for an explicitly retained token during asynchronous presentation.
/// @details The token is borrowed and must remain within its SDK lifetime; this call does not replace current frame state.
/// @return SDK result, or -1 for unavailable markers/a zero token.
RSF_SL_VK_EXPORT int VulkanStorySlMarkerForToken(uintptr_t token, uint32_t marker) {
    auto* frame = reinterpret_cast<sl::FrameToken*>(token);
    return pclMarker && frame ? static_cast<int>(pclMarker(static_cast<sl::PCLMarker>(marker), *frame)) : -1;
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
RSF_SL_VK_EXPORT int VulkanStorySlTagFrame(VkCommandBuffer commandBuffer,
    const TaggedImage* depth, const TaggedImage* motion, const TaggedImage* hudless,
    const TaggedImage* ui, const FrameCamera* camera) {
    if (!setTagForFrame || !setConstants || !frameToken || !depth || !motion ||
        !hudless || !ui || !camera || !camera->viewToClip || !camera->clipToView ||
        !camera->clipToPrevClip || !camera->prevClipToClip) return -1;

    sl::Constants constants{};
    rowMajor(constants.cameraViewToClip, camera->viewToClip);
    rowMajor(constants.clipToCameraView, camera->clipToView);
    rowMajor(constants.clipToPrevClip, camera->clipToPrevClip);
    rowMajor(constants.prevClipToClip, camera->prevClipToClip);
    constants.jitterOffset = {camera->jitterX, camera->jitterY};
    constants.mvecScale = {camera->mvecScaleX, camera->mvecScaleY};
    // The game camera has no lens/pinhole shift; jitter is supplied separately.
    constants.cameraPinholeOffset = {0.f, 0.f};
    constants.cameraPos = {camera->position[0], camera->position[1], camera->position[2]};
    constants.cameraUp = {camera->up[0], camera->up[1], camera->up[2]};
    constants.cameraRight = {camera->right[0], camera->right[1], camera->right[2]};
    constants.cameraFwd = {camera->forward[0], camera->forward[1], camera->forward[2]};
    constants.cameraNear = camera->nearPlane;
    constants.cameraFar = camera->farPlane;
    constants.cameraFOV = camera->fov;
    constants.cameraAspectRatio = camera->aspect;
    constants.depthInverted = sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue;
    constants.motionVectors3D = sl::Boolean::eFalse;
    constants.reset = camera->reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    auto result = setConstants(constants, *frameToken, viewport);
    if (result != sl::Result::eOk) return static_cast<int>(result);

    sl::Resource d = resource(*depth), m = resource(*motion), h = resource(*hudless), u = resource(*ui);
    const sl::Extent depthExtent{0, 0, depth->width, depth->height};
    const sl::Extent motionExtent{0, 0, motion->width, motion->height};
    const sl::Extent colorExtent{0, 0, hudless->width, hudless->height};
    const sl::Extent uiExtent{0, 0, ui->width, ui->height};
      // The renderer reuses its four upright staging images on the next frame,
      // which can begin before this frame reaches the asynchronous Present.
      // Copy them on the tagging command buffer so each Present sees its own
      // depth, motion, scene and UI contents.
      sl::ResourceTag tags[] = {
          {&d, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow, &depthExtent},
          {&m, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eOnlyValidNow, &motionExtent},
          {&h, sl::kBufferTypeHUDLessColor, sl::ResourceLifecycle::eOnlyValidNow, &colorExtent},
          {&u, sl::kBufferTypeUIColorAndAlpha, sl::ResourceLifecycle::eOnlyValidNow, &uiExtent},
      };
    return static_cast<int>(setTagForFrame(*frameToken, viewport, tags, 4,
        reinterpret_cast<sl::CommandBuffer*>(commandBuffer)));
}
/// @brief Clears scene resource tags for the current frame token.
/// @details Use at non-scene frames or before resources/provider state change. The SDK resolves the full backbuffer after deferred swapchain recreation; no client subregion is supplied.
/// @return SDK result, or -1 without tagging/token availability.
RSF_SL_VK_EXPORT int VulkanStorySlInvalidateFrameTags() {
    if (!setTagForFrame || !frameToken) return -1;
    sl::ResourceTag tags[] = {
        {nullptr, sl::kBufferTypeDepth, sl::ResourceLifecycle::eValidUntilPresent, nullptr},
        {nullptr, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eValidUntilPresent, nullptr},
        {nullptr, sl::kBufferTypeHUDLessColor, sl::ResourceLifecycle::eValidUntilPresent, nullptr},
        {nullptr, sl::kBufferTypeUIColorAndAlpha, sl::ResourceLifecycle::eValidUntilPresent, nullptr},
    };
    return static_cast<int>(setTagForFrame(*frameToken, viewport, tags, 4, nullptr));
}
/// @brief Applies DLSS-G enablement, interpolation count and presentation-resource metadata.
/// @details Successful identical options are reused. An Off out-of-VRAM warning clears configuration without proving release; the host must drain and explicitly free resources.
/// @return SDK result, -1 without options or -2 for an enabled zero interpolation count.
RSF_SL_VK_EXPORT int VulkanStorySlSetFrameGeneration(int enabled, uint32_t count,
    uint32_t width, uint32_t height, uint32_t colorFormat, uint32_t backBuffers) {
    // Swapchain recreation invalidates the dimensions/options cache, not the
    // feature's enabled state. Off is also the SDK default. Do not re-submit
    // Off for an unused/already-disabled feature when another provider rebuilds
    // presentation; Streamline can observe both requests before one Present.
    // A subsequent On still supplies the new swapchain description below.
    if (!enabled && !fgEnabled) return 0;
    if (!fgOptions) return -1;
    if (enabled && count == 0) return -2;
    if (fgConfigured && fgEnabled == (enabled != 0) && fgCount == count &&
        fgWidth == width && fgHeight == height && fgFormat == colorFormat &&
        fgBackBuffers == backBuffers) return 0;
    sl::DLSSGOptions options{};
    options.mode = enabled ? sl::DLSSGMode::eOn : sl::DLSSGMode::eOff;
    options.numFramesToGenerate = enabled ? count : 1;
    options.colorWidth = width; options.colorHeight = height;
    options.colorBufferFormat = colorFormat; options.numBackBuffers = backBuffers;
    options.enableUserInterfaceRecomposition = sl::Boolean::eTrue;
    options.onErrorCallback = onPresentError;
    int result = static_cast<int>(fgOptions(viewport, options));
    if (!enabled && result == static_cast<int>(sl::Result::eWarnOutOfVRAM)) {
        // Do not cache Off as a successful configuration. Allow the host's
        // checked device drain + explicit slFreeResources path to release the
        // feature instead of throwing while disposing framebuffer inputs.
        fgEnabled = false;
        fgConfigured = false;
    }
    if (result == 0) {
        fgConfigured = true;
        fgEnabled = enabled != 0;
        if (fgEnabled) fgResourcesMayExist = true;
        fgCount = count;
        fgWidth = width; fgHeight = height; fgFormat = colorFormat;
        fgBackBuffers = backBuffers;
    }
    return result;
}
// Called after FG is off and the proxy's DeviceWaitIdle has drained presentation.
// Direct NGX SR shutdown must not precede release of Streamline's FG handle.
/// @brief Disables a previously enabled DLSS-G feature before the host's checked drain/release sequence.
/// @details Unused or already disabled features return zero without configuring another provider's presentation.
/// @return SDK option result, or zero when no disable is required.
RSF_SL_VK_EXPORT int VulkanStorySlDisableFrameGenerationForRelease() {
    // A different provider may own presentation. Do not configure an unused
    // DLSS-G feature solely to shut it down, or repeat an already-applied Off.
    if (!fgResourcesMayExist || !fgEnabled) return 0;
    return VulkanStorySlSetFrameGeneration(0, fgCount, fgWidth, fgHeight, fgFormat, fgBackBuffers);
}
/// @brief Frees known DLSS-G resources after enablement is Off and the host has drained proxy GPU/present work.
/// @details Resource-ownership state clears only when slFreeResources succeeds.
/// @return SDK result, zero for no resources, or -1 when release prerequisites are missing.
RSF_SL_VK_EXPORT int VulkanStorySlFreeFrameGenerationResources() {
    if (!fgOptions || !fgResourcesMayExist) return 0;
    if (!freeResources || fgEnabled) return -1;
    const auto result = freeResources(sl::kFeatureDLSS_G, viewport);
    if (result == sl::Result::eOk) { fgConfigured = false; fgResourcesMayExist = false; }
    return static_cast<int>(result);
}
/// @brief Atomically consumes the latest recorded asynchronous presentation error.
/// @return Recorded Vulkan error code, or zero when none is pending.
RSF_SL_VK_EXPORT int VulkanStorySlTakePresentError() {
    return lastPresentError.exchange(0, std::memory_order_relaxed);
}
/// @brief Queries frame-generation status, actual presents and the generated-frame limit.
/// @details Output values are written only after a successful SDK state query.
/// @return SDK result, or -1 for unavailable state/null outputs.
RSF_SL_VK_EXPORT int VulkanStorySlGetFrameGenerationState(uint32_t* status,
    uint32_t* presented, uint32_t* maxGenerated) {
    if (!fgState || !status || !presented || !maxGenerated) return -1;
    sl::DLSSGState state{};
    auto result = fgState(viewport, state, nullptr);
    if (result == sl::Result::eOk) {
        *status = static_cast<uint32_t>(state.status);
        *presented = state.numFramesActuallyPresented;
        *maxGenerated = state.numFramesToGenerateMax;
    }
    return static_cast<int>(result);
}
/// @brief Queries the six-word frame-generation capability/state ABI.
/// @details Words contain status, actual presents, maximum generated count, minimum size, VSync availability and dynamic-MFG support in that order.
/// @param values Caller-owned output array containing exactly six uint32_t values.
/// @param count Must equal six.
/// @return SDK result, or -1 for unavailable state/invalid output storage.
RSF_SL_VK_EXPORT int VulkanStorySlGetFrameGenerationStateDetails(uint32_t* values, uint32_t count) {
    if (!fgState || !values || count != 6) return -1;
    sl::DLSSGState state{};
    const auto result = fgState(viewport, state, nullptr);
    if (result == sl::Result::eOk) {
        values[0] = static_cast<uint32_t>(state.status);
        values[1] = state.numFramesActuallyPresented;
        values[2] = state.numFramesToGenerateMax;
        values[3] = state.minWidthOrHeight;
        values[4] = static_cast<uint32_t>(state.bIsVsyncSupportAvailable);
        values[5] = static_cast<uint32_t>(state.bIsDynamicMFGSupported);
    }
    return static_cast<int>(result);
}
}
