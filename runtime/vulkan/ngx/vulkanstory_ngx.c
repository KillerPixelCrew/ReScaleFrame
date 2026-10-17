/*
 * VulkanStory's NGX shim. See vulkanstory_ngx.h for why it exists and what it promises.
 *
 * Plain C99, no C++ runtime, no NGX headers, no link-time dependency on the
 * NVIDIA driver: the runtime is dlopen()ed and every entry point dlsym()ed at
 * first use, so this object builds and loads on any machine.
 *
 *   cc -std=c99 -O2 -fPIC -shared -fvisibility=hidden -fno-optimize-sibling-calls \
 *      native/ngx/vulkanstory_ngx.c -o libVulkanStoryNgx.so -ldl
 *
 * Use native/ngx/build.sh on Linux or scripts/build-provider-bridges.ps1 on
 * Windows. The renderer project does not build or copy this shim implicitly.
 */
#include "vulkanstory_ngx.h"

#include <stddef.h>
#include <string.h>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <wchar.h>
   typedef HMODULE vulkanstory_module;
#  define VULKANSTORY_NGX_RUNTIME "nvngx.dll"
#else
#  include <dlfcn.h>
   typedef void *vulkanstory_module;
#  define VULKANSTORY_NGX_RUNTIME "libnvidia-ngx.so.1"
#endif

/* NVSDK_NGX_Result_Success. Success is 1, not 0. */
#define NGX_SUCCESS 1u

/* ------------------------------------------------------------------ loading */

/*
 * The entry points the managed side needs, by exported name. Everything is
 * resolved once; a symbol the runtime does not export stays null and its
 * wrapper answers VULKANSTORY_NGX_RESULT_ENTRY_POINT_MISSING instead of crashing.
 */
#define VULKANSTORY_NGX_ENTRY_POINTS(X)                                                    \
    X(init_project_id,      "NVSDK_NGX_VULKAN_Init_ProjectID")                         \
    X(shutdown1,            "NVSDK_NGX_VULKAN_Shutdown1")                              \
    X(get_capability_params,"NVSDK_NGX_VULKAN_GetCapabilityParameters")                \
    X(allocate_params,      "NVSDK_NGX_VULKAN_AllocateParameters")                     \
    X(destroy_params,       "NVSDK_NGX_VULKAN_DestroyParameters")                      \
    X(feature_requirements, "NVSDK_NGX_VULKAN_GetFeatureRequirements")                 \
    X(instance_extensions,  "NVSDK_NGX_VULKAN_GetFeatureInstanceExtensionRequirements")\
    X(device_extensions,    "NVSDK_NGX_VULKAN_GetFeatureDeviceExtensionRequirements")  \
    X(create_feature,       "NVSDK_NGX_VULKAN_CreateFeature")                          \
    X(create_feature1,      "NVSDK_NGX_VULKAN_CreateFeature1")                         \
    X(evaluate_feature,     "NVSDK_NGX_VULKAN_EvaluateFeature")                        \
    X(release_feature,      "NVSDK_NGX_VULKAN_ReleaseFeature")

/// @brief Process-retained addresses for the NGX runtime exports used by this shim.
/// @details A missing symbol remains null and its wrapper returns the shim entry-point-missing result.
struct vulkanstory_ngx_entries
{
#define VULKANSTORY_NGX_DECLARE(field, name) void *field;
    VULKANSTORY_NGX_ENTRY_POINTS(VULKANSTORY_NGX_DECLARE)
#undef VULKANSTORY_NGX_DECLARE
};

enum { LOAD_UNTRIED = 0, LOAD_RUNNING = 1, LOAD_READY = 2, LOAD_FAILED = 3 };

static int g_load_state = LOAD_UNTRIED;
static vulkanstory_module g_runtime = NULL;
static struct vulkanstory_ngx_entries g_ngx;
static char g_load_error[512] = { 0 };

/// @brief Resolves an exported function in the borrowed native runtime module.
static void *vulkanstory_symbol(vulkanstory_module module, const char *name)
{
#if defined(_WIN32)
    return (void *)GetProcAddress(module, name);
#else
    return dlsym(module, name);
#endif
}

/// @brief Copies the native loader failure into bounded process-retained diagnostic storage.
static void vulkanstory_record_load_error(void)
{
#if defined(_WIN32)
    unsigned long code = GetLastError();
    /* No FormatMessage: a number is enough to tell "not found" from "bad image". */
    const char prefix[] = "LoadLibrary failed, GetLastError=";
    size_t at = sizeof prefix - 1;
    memcpy(g_load_error, prefix, at);
    if (at + 12 < sizeof g_load_error)
    {
        char digits[12];
        int n = 0;
        do { digits[n++] = (char)('0' + (code % 10u)); code /= 10u; } while (code != 0 && n < 11);
        while (n > 0) g_load_error[at++] = digits[--n];
    }
    g_load_error[at] = '\0';
#else
    const char *message = dlerror();
    if (message == NULL) message = "the runtime could not be loaded";
    strncpy(g_load_error, message, sizeof g_load_error - 1);
    g_load_error[sizeof g_load_error - 1] = '\0';
#endif
}

#if defined(_WIN32)
/*
 * Windows display drivers keep nvngx.dll in DriverStore rather than System32.
 * LoadLibrary("nvngx.dll") alone fails on a normal NVIDIA install. Search the
 * installed NVIDIA driver directories as a fallback, without hard-coding a
 * machine-specific INF hash or bundling the driver's own DLL with the game.
 */
/// @brief Loads nvngx.dll from normal discovery or installed NVIDIA DriverStore directories.
/// @details The successful module stays loaded for the process; this helper does not bundle or modify driver files.
static vulkanstory_module vulkanstory_load_windows_runtime(void)
{
    vulkanstory_module runtime = LoadLibraryA(VULKANSTORY_NGX_RUNTIME);
    if (runtime != NULL) return runtime;

    wchar_t system_dir[MAX_PATH];
    UINT system_len = GetSystemDirectoryW(system_dir, MAX_PATH);
    if (system_len == 0 || system_len >= MAX_PATH) return NULL;

    wchar_t pattern[MAX_PATH];
    int length = swprintf(pattern, MAX_PATH,
                          L"%ls\\DriverStore\\FileRepository\\nv*.inf_amd64_*", system_dir);
    if (length < 0 || length >= MAX_PATH) return NULL;

    WIN32_FIND_DATAW entry;
    HANDLE search = FindFirstFileW(pattern, &entry);
    if (search == INVALID_HANDLE_VALUE) return NULL;
    do
    {
        if ((entry.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0) continue;
        wchar_t candidate[MAX_PATH];
        length = swprintf(candidate, MAX_PATH,
                          L"%ls\\DriverStore\\FileRepository\\%ls\\nvngx.dll",
                          system_dir, entry.cFileName);
        if (length < 0 || length >= MAX_PATH) continue;
        runtime = LoadLibraryExW(candidate, NULL, LOAD_WITH_ALTERED_SEARCH_PATH);
        if (runtime != NULL) break;
    } while (FindNextFileW(search, &entry));
    FindClose(search);
    return runtime;
}
#endif

/*
 * Loads once. A second thread arriving mid-load spins on the state word rather
 * than taking a lock, which keeps this file free of pthread and of any
 * platform-specific synchronisation; the load happens once at renderer start.
 */
/// @brief Serializes the one-time NGX module load and publishes success/failure through the atomic load-state word.
/// @details Concurrent callers spin while loading is in progress. A published failure is terminal for this shim instance.
static int vulkanstory_ensure_runtime(void)
{
    int state = __atomic_load_n(&g_load_state, __ATOMIC_ACQUIRE);
    while (state == LOAD_RUNNING)
    {
        state = __atomic_load_n(&g_load_state, __ATOMIC_ACQUIRE);
    }
    if (state == LOAD_READY) return 1;
    if (state == LOAD_FAILED) return 0;

    int expected = LOAD_UNTRIED;
    if (!__atomic_compare_exchange_n(&g_load_state, &expected, LOAD_RUNNING, 0,
                                     __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE))
    {
        /* Someone else got there first; re-enter and observe their result. */
        return vulkanstory_ensure_runtime();
    }

#if defined(_WIN32)
    g_runtime = vulkanstory_load_windows_runtime();
#else
    dlerror();
    g_runtime = dlopen(VULKANSTORY_NGX_RUNTIME, RTLD_NOW | RTLD_LOCAL);
#endif
    if (g_runtime == NULL)
    {
        vulkanstory_record_load_error();
        __atomic_store_n(&g_load_state, LOAD_FAILED, __ATOMIC_RELEASE);
        return 0;
    }

#define VULKANSTORY_NGX_RESOLVE(field, name) g_ngx.field = vulkanstory_symbol(g_runtime, name);
    VULKANSTORY_NGX_ENTRY_POINTS(VULKANSTORY_NGX_RESOLVE)
#undef VULKANSTORY_NGX_RESOLVE

    __atomic_store_n(&g_load_state, LOAD_READY, __ATOMIC_RELEASE);
    return 1;
}

/* Every wrapper starts with this: no runtime, no symbol, no call. */
#define VULKANSTORY_NGX_REQUIRE(field)                                       \
    if (!vulkanstory_ensure_runtime()) return VULKANSTORY_NGX_RESULT_RUNTIME_MISSING; \
    if (g_ngx.field == NULL) return VULKANSTORY_NGX_RESULT_ENTRY_POINT_MISSING;

/*
 * ...and every wrapper *ends* with this, which is the whole point of the file.
 *
 * `return f(args);` compiles to a tail call (`jmp *%rax`) at -O2: the wrapper
 * pops its own frame first, so the return address NGX reads is not this shared
 * object's - it is the managed caller's JIT stub, and NGX aborts exactly as it
 * does without a shim. Measured, not feared: the first build of this file
 * forwarded with plain `return` and the managed test still died with
 * "basic_string::_M_construct null not valid" (2026-09-12).
 *
 * Writing the result to a volatile local after the call forbids the tail call
 * in the language rather than in a build flag, so the guarantee survives being
 * compiled with different flags or a different compiler. build.sh also passes
 * -fno-optimize-sibling-calls, belt and braces.
 */
#define VULKANSTORY_NGX_RETURN(expression)             \
    do {                                           \
        volatile VulkanStoryNgxResult _result = (expression); \
        return _result;                            \
    } while (0)

uint32_t VulkanStoryNgx_Version(void) { return VULKANSTORY_NGX_SHIM_VERSION; }

const char *VulkanStoryNgx_RuntimeName(void) { return VULKANSTORY_NGX_RUNTIME; }

const char *VulkanStoryNgx_LastLoadError(void) { return g_load_error; }

VulkanStoryNgxResult VulkanStoryNgx_LoadRuntime(void)
{
    return vulkanstory_ensure_runtime() ? NGX_SUCCESS : VULKANSTORY_NGX_RESULT_RUNTIME_MISSING;
}

/* ----------------------------------------------------------------- lifetime */

typedef VulkanStoryNgxResult (*pfn_init_project_id)(
    const char *, int, const char *, const void *, void *, void *, void *, int, const void *);
typedef VulkanStoryNgxResult (*pfn_handle)(void *);
/*
 * NVSDK_NGX_VULKAN_Shutdown1 as the driver really implements it.
 *
 * The public header (nvsdk_ngx_vk.h) declares one parameter, VkDevice. The
 * implementation in libnvidia-ngx.so.1 (driver 615.71.09) takes two: it moves
 * its *second* argument straight into the fourth argument of the internal
 * shutdown routine, and that routine stores the SDK's remaining reference count
 * through it without ever testing it for NULL:
 *
 *     NVSDK_NGX_VULKAN_Shutdown1:  mov %rsi,%rcx  ...  jmp <internal shutdown>
 *     internal shutdown:           mov %rcx,0x10(%rsp)
 *                                  ...
 *                                  mov 0x10(%rsp),%rsi
 *                                  mov %eax,(%rsi)      <-- the SIGSEGV
 *
 * The deprecated one-argument NVSDK_NGX_VULKAN_Shutdown passes `lea 0xc(%rsp)`
 * there, which is what the count is meant to land in. Called through the
 * header's prototype, %rsi holds whatever the caller happened to leave in it -
 * a writable address by luck (a C test harness), or 8192 (the .NET client and
 * the test host, every time), and then NGX writes four bytes to address 0x2000
 * and the process dies inside the driver. That is the crash both core dumps
 * show, on the *first* Shutdown1 of the process.
 *
 * So the shim always passes a real int* it owns. An extra register argument is
 * harmless if a future driver really does take one parameter, and it is the
 * only thing that makes the call defined on this one.
 */
typedef VulkanStoryNgxResult (*pfn_shutdown1)(void *, int *);
typedef VulkanStoryNgxResult (*pfn_out_pointer)(void **);

VulkanStoryNgxResult VulkanStoryNgx_VulkanInitProjectId(
    const char *projectId, int engineType, const char *engineVersion,
    const void *applicationDataPath, void *instance, void *physicalDevice, void *device,
    int sdkVersion, const void *featureCommonInfo)
{
    VULKANSTORY_NGX_REQUIRE(init_project_id)
    if (projectId == NULL || device == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_init_project_id)g_ngx.init_project_id)(
        projectId, engineType, engineVersion, applicationDataPath,
        instance, physicalDevice, device, sdkVersion, featureCommonInfo));
}

VulkanStoryNgxResult VulkanStoryNgx_VulkanShutdown(void *device)
{
    /* Written by NGX with the SDK's remaining reference count; see pfn_shutdown1. */
    int remainingReferences = 0;
    VULKANSTORY_NGX_REQUIRE(shutdown1)
    VULKANSTORY_NGX_RETURN(((pfn_shutdown1)g_ngx.shutdown1)(device, &remainingReferences));
}

/* --------------------------------------------------------- parameter blocks */

VulkanStoryNgxResult VulkanStoryNgx_GetCapabilityParameters(void **outParameters)
{
    VULKANSTORY_NGX_REQUIRE(get_capability_params)
    if (outParameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outParameters = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_out_pointer)g_ngx.get_capability_params)(outParameters));
}

VulkanStoryNgxResult VulkanStoryNgx_AllocateParameters(void **outParameters)
{
    VULKANSTORY_NGX_REQUIRE(allocate_params)
    if (outParameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outParameters = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_out_pointer)g_ngx.allocate_params)(outParameters));
}

VulkanStoryNgxResult VulkanStoryNgx_DestroyParameters(void *parameters)
{
    VULKANSTORY_NGX_REQUIRE(destroy_params)
    if (parameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_handle)g_ngx.destroy_params)(parameters));
}

/* ---------------------------------------------------------------- discovery */

typedef VulkanStoryNgxResult (*pfn_feature_requirements)(void *, void *, const void *, void *);
typedef VulkanStoryNgxResult (*pfn_instance_extensions)(const void *, uint32_t *, void **);
typedef VulkanStoryNgxResult (*pfn_device_extensions)(void *, void *, const void *, uint32_t *, void **);

VulkanStoryNgxResult VulkanStoryNgx_GetFeatureRequirements(
    void *instance, void *physicalDevice, const void *discovery, void *outRequirement)
{
    VULKANSTORY_NGX_REQUIRE(feature_requirements)
    if (discovery == NULL || outRequirement == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_feature_requirements)g_ngx.feature_requirements)(
        instance, physicalDevice, discovery, outRequirement));
}

VulkanStoryNgxResult VulkanStoryNgx_GetFeatureInstanceExtensionRequirements(
    const void *discovery, uint32_t *outCount, void **outExtensionProperties)
{
    VULKANSTORY_NGX_REQUIRE(instance_extensions)
    if (discovery == NULL || outCount == NULL || outExtensionProperties == NULL)
        return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outCount = 0;
    *outExtensionProperties = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_instance_extensions)g_ngx.instance_extensions)(
        discovery, outCount, outExtensionProperties));
}

VulkanStoryNgxResult VulkanStoryNgx_GetFeatureDeviceExtensionRequirements(
    void *instance, void *physicalDevice, const void *discovery,
    uint32_t *outCount, void **outExtensionProperties)
{
    VULKANSTORY_NGX_REQUIRE(device_extensions)
    if (discovery == NULL || outCount == NULL || outExtensionProperties == NULL)
        return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outCount = 0;
    *outExtensionProperties = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_device_extensions)g_ngx.device_extensions)(
        instance, physicalDevice, discovery, outCount, outExtensionProperties));
}

/* ------------------------------------------------------ parameter accessors */

/*
 * NVSDK_NGX_Parameter's vtable, in the declaration order of
 * nvsdk_ngx_params.h. The Linux driver uses declaration order (Itanium ABI),
 * while the Windows SDK's MSVC wrappers call the overloads in reverse order.
 * The Windows offsets below match nvsdk_ngx_s.lib from DLSS SDK 310.9.1.
 */
enum
{
#if defined(_WIN32)
    SLOT_SET_ULL   = 7,
    SLOT_SET_F     = 6,
    SLOT_SET_D     = 5,
    SLOT_SET_UI    = 4,
    SLOT_SET_I     = 3,
    SLOT_SET_D3D11 = 2,
    SLOT_SET_D3D12 = 1,
    SLOT_SET_VP    = 0,
    SLOT_GET_ULL   = 15,
    SLOT_GET_F     = 14,
    SLOT_GET_D     = 13,
    SLOT_GET_UI    = 12,
    SLOT_GET_I     = 11,
    SLOT_GET_D3D11 = 10,
    SLOT_GET_D3D12 = 9,
    SLOT_GET_VP    = 8
#else
    SLOT_SET_ULL   = 0,
    SLOT_SET_F     = 1,
    SLOT_SET_D     = 2,
    SLOT_SET_UI    = 3,
    SLOT_SET_I     = 4,
    SLOT_SET_D3D11 = 5,
    SLOT_SET_D3D12 = 6,
    SLOT_SET_VP    = 7,
    SLOT_GET_ULL   = 8,
    SLOT_GET_F     = 9,
    SLOT_GET_D     = 10,
    SLOT_GET_UI    = 11,
    SLOT_GET_I     = 12,
    SLOT_GET_D3D11 = 13,
    SLOT_GET_D3D12 = 14,
    SLOT_GET_VP    = 15
#endif
};

#define VTABLE(p) (*(void ***)(p))

/// @brief Generates typed parameter-set wrappers with ABI-specific vtable slots and null-argument checks.
#define VULKANSTORY_NGX_SETTER(suffix, ctype, slot)                                  \
    VulkanStoryNgxResult VulkanStoryNgx_ParameterSet##suffix(                            \
        void *p, const char *name, ctype value)                                  \
    {                                                                            \
        typedef void (*setter)(void *, const char *, ctype);                      \
        if (p == NULL || name == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;\
        ((setter)VTABLE(p)[slot])(p, name, value);                               \
        /* not a tail call: a value is returned after it. */                      \
        return NGX_SUCCESS;                                                      \
    }

/// @brief Generates typed parameter-get wrappers that preserve the native call-site return address.
#define VULKANSTORY_NGX_GETTER(suffix, ctype, slot)                                  \
    VulkanStoryNgxResult VulkanStoryNgx_ParameterGet##suffix(                            \
        void *p, const char *name, ctype *value)                                 \
    {                                                                            \
        typedef VulkanStoryNgxResult (*getter)(void *, const char *, ctype *);        \
        if (p == NULL || name == NULL || value == NULL)                          \
            return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;                          \
        VULKANSTORY_NGX_RETURN(((getter)VTABLE(p)[slot])(p, name, value));           \
    }

VULKANSTORY_NGX_SETTER(ULongLong, uint64_t, SLOT_SET_ULL)
VULKANSTORY_NGX_SETTER(Float, float, SLOT_SET_F)
VULKANSTORY_NGX_SETTER(Double, double, SLOT_SET_D)
VULKANSTORY_NGX_SETTER(UInt, uint32_t, SLOT_SET_UI)
VULKANSTORY_NGX_SETTER(Int, int32_t, SLOT_SET_I)
VULKANSTORY_NGX_SETTER(VoidPointer, void *, SLOT_SET_VP)

VULKANSTORY_NGX_GETTER(ULongLong, uint64_t, SLOT_GET_ULL)
VULKANSTORY_NGX_GETTER(Float, float, SLOT_GET_F)
VULKANSTORY_NGX_GETTER(Double, double, SLOT_GET_D)
VULKANSTORY_NGX_GETTER(UInt, uint32_t, SLOT_GET_UI)
VULKANSTORY_NGX_GETTER(Int, int32_t, SLOT_GET_I)

VulkanStoryNgxResult VulkanStoryNgx_ParameterGetVoidPointer(void *p, const char *name, void **value)
{
    typedef VulkanStoryNgxResult (*getter)(void *, const char *, void **);
    if (p == NULL || name == NULL || value == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *value = NULL;
    VULKANSTORY_NGX_RETURN(((getter)VTABLE(p)[SLOT_GET_VP])(p, name, value));
}

/* ----------------------------------------------------------------- features */

typedef VulkanStoryNgxResult (*pfn_create_feature)(void *, int, void *, void **);
typedef VulkanStoryNgxResult (*pfn_create_feature1)(void *, void *, int, void *, void **);
typedef VulkanStoryNgxResult (*pfn_evaluate_feature)(void *, void *, void *, void *);

VulkanStoryNgxResult VulkanStoryNgx_CreateFeature(
    void *commandBuffer, int featureId, void *parameters, void **outHandle)
{
    VULKANSTORY_NGX_REQUIRE(create_feature)
    if (parameters == NULL || outHandle == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outHandle = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_create_feature)g_ngx.create_feature)(
        commandBuffer, featureId, parameters, outHandle));
}

VulkanStoryNgxResult VulkanStoryNgx_CreateFeature1(
    void *device, void *commandBuffer, int featureId, void *parameters, void **outHandle)
{
    VULKANSTORY_NGX_REQUIRE(create_feature1)
    if (parameters == NULL || outHandle == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outHandle = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_create_feature1)g_ngx.create_feature1)(
        device, commandBuffer, featureId, parameters, outHandle));
}

VulkanStoryNgxResult VulkanStoryNgx_EvaluateFeature(
    void *commandBuffer, void *handle, void *parameters, void *progressCallback)
{
    VULKANSTORY_NGX_REQUIRE(evaluate_feature)
    if (handle == NULL || parameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_evaluate_feature)g_ngx.evaluate_feature)(
        commandBuffer, handle, parameters, progressCallback));
}

VulkanStoryNgxResult VulkanStoryNgx_ReleaseFeature(void *handle)
{
    VULKANSTORY_NGX_REQUIRE(release_feature)
    if (handle == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_handle)g_ngx.release_feature)(handle));
}

/* --------------------------------------------------- DLSS optimal settings */

/* NVSDK_NGX_Result_FAIL_OutOfDate, what the header's helper returns with no callback. */
#define NGX_FAIL_OUT_OF_DATE 0xBAD0000Cu

/// @brief Reads a typed NGX uint parameter or uses the supplied fallback when the accessor fails.
static uint32_t vulkanstory_get_uint_or(void *p, const char *name, uint32_t fallback)
{
    uint32_t value = 0;
    VulkanStoryNgxResult result = VulkanStoryNgx_ParameterGetUInt(p, name, &value);
    return (result == NGX_SUCCESS) ? value : fallback;
}

VulkanStoryNgxResult VulkanStoryNgx_DlssGetOptimalSettings(
    void *parameters, uint32_t displayWidth, uint32_t displayHeight, int perfQuality,
    uint32_t *outOptimalWidth, uint32_t *outOptimalHeight,
    uint32_t *outMinWidth, uint32_t *outMinHeight,
    uint32_t *outMaxWidth, uint32_t *outMaxHeight,
    float *outSharpness)
{
    typedef VulkanStoryNgxResult (*pfn_optimal)(void *);

    if (parameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;

    void *callback = NULL;
    VulkanStoryNgxResult lookup =
        VulkanStoryNgx_ParameterGetVoidPointer(parameters, "DLSSOptimalSettingsCallback", &callback);
    if (callback == NULL) return (lookup == NGX_SUCCESS) ? NGX_FAIL_OUT_OF_DATE : lookup;

    VulkanStoryNgx_ParameterSetUInt(parameters, "Width", displayWidth);
    VulkanStoryNgx_ParameterSetUInt(parameters, "Height", displayHeight);
    VulkanStoryNgx_ParameterSetInt(parameters, "PerfQualityValue", perfQuality);
    VulkanStoryNgx_ParameterSetInt(parameters, "RTXValue", 0);
    /* The callback lives inside libnvidia-ngx, so it reads the return address
       the same way; the volatile keeps this frame alive across it. */
    volatile VulkanStoryNgxResult called = ((pfn_optimal)callback)(parameters);
    VulkanStoryNgxResult result = called;
    if (result != NGX_SUCCESS) return result;

    uint32_t optimalWidth = vulkanstory_get_uint_or(parameters, "OutWidth", 0);
    uint32_t optimalHeight = vulkanstory_get_uint_or(parameters, "OutHeight", 0);

    if (outOptimalWidth) *outOptimalWidth = optimalWidth;
    if (outOptimalHeight) *outOptimalHeight = optimalHeight;
    if (outMinWidth) *outMinWidth = vulkanstory_get_uint_or(parameters, "DLSS.Get.Dynamic.Min.Render.Width", optimalWidth);
    if (outMinHeight) *outMinHeight = vulkanstory_get_uint_or(parameters, "DLSS.Get.Dynamic.Min.Render.Height", optimalHeight);
    if (outMaxWidth) *outMaxWidth = vulkanstory_get_uint_or(parameters, "DLSS.Get.Dynamic.Max.Render.Width", optimalWidth);
    if (outMaxHeight) *outMaxHeight = vulkanstory_get_uint_or(parameters, "DLSS.Get.Dynamic.Max.Render.Height", optimalHeight);
    if (outSharpness)
    {
        float sharpness = 0.0f;
        if (VulkanStoryNgx_ParameterGetFloat(parameters, "Sharpness", &sharpness) != NGX_SUCCESS) sharpness = 0.0f;
        *outSharpness = sharpness;
    }
    return result;
}
