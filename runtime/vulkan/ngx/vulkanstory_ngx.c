/*
 * VulkanStory's NGX shim. See vulkanstory_ngx.h for why it exists and what it promises.
 *
 * The native wrappers call NGX from this shared object. Rust owns the lazy
 * load result and symbol table. There is no link-time dependency on NGX.
 * Build the shim with the adjacent build.sh or build.ps1.
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

/* Private ABI, matching NgxEntries in src/lib.rs. Missing symbols stay null. */
struct vulkanstory_ngx_entries
{
    void *init_project_id;
    void *shutdown1;
    void *get_capability_params;
    void *allocate_params;
    void *destroy_params;
    void *feature_requirements;
    void *instance_extensions;
    void *device_extensions;
    void *create_feature;
    void *create_feature1;
    void *evaluate_feature;
    void *release_feature;
};

extern const struct vulkanstory_ngx_entries *rsf_vulkanstory_ngx_get_entries(void);
extern const char *rsf_vulkanstory_ngx_load_error(void);

/// @brief Resolves an exported function in the borrowed native runtime module.
void *rsf_vulkanstory_ngx_symbol(void *module, const char *name)
{
#if defined(_WIN32)
    return (void *)GetProcAddress((vulkanstory_module)module, name);
#else
    return dlsym(module, name);
#endif
}

/// @brief Copies the native loader failure into the caller's diagnostic buffer.
static void vulkanstory_record_load_error(char *error, size_t capacity)
{
#if defined(_WIN32)
    unsigned long code = GetLastError();
    /* No FormatMessage: a number is enough to tell "not found" from "bad image". */
    const char prefix[] = "LoadLibrary failed, GetLastError=";
    size_t at = sizeof prefix - 1;
    memcpy(error, prefix, at);
    if (at + 12 < capacity)
    {
        char digits[12];
        int n = 0;
        do { digits[n++] = (char)('0' + (code % 10u)); code /= 10u; } while (code != 0 && n < 11);
        while (n > 0) error[at++] = digits[--n];
    }
    error[at] = '\0';
#else
    const char *message = dlerror();
    if (message == NULL) message = "the runtime could not be loaded";
    strncpy(error, message, capacity - 1);
    error[capacity - 1] = '\0';
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

/* Rust supplies a writable 512-byte buffer for any native loader error. */
void *rsf_vulkanstory_ngx_open(char *error, size_t capacity)
{
#if defined(_WIN32)
    vulkanstory_module runtime = vulkanstory_load_windows_runtime();
#else
    dlerror();
    vulkanstory_module runtime = dlopen(VULKANSTORY_NGX_RUNTIME, RTLD_NOW | RTLD_LOCAL);
#endif
    if (runtime == NULL) vulkanstory_record_load_error(error, capacity);
    return runtime;
}

/* Every wrapper starts with this: no runtime, no symbol, no call. */
#define VULKANSTORY_NGX_REQUIRE(field)                                                      \
    const struct vulkanstory_ngx_entries *ngx = rsf_vulkanstory_ngx_get_entries();             \
    if (ngx == NULL) return VULKANSTORY_NGX_RESULT_RUNTIME_MISSING;                          \
    if (ngx->field == NULL) return VULKANSTORY_NGX_RESULT_ENTRY_POINT_MISSING;

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

const char *VulkanStoryNgx_LastLoadError(void) { return rsf_vulkanstory_ngx_load_error(); }

VulkanStoryNgxResult VulkanStoryNgx_LoadRuntime(void)
{
    return rsf_vulkanstory_ngx_get_entries() != NULL ? NGX_SUCCESS : VULKANSTORY_NGX_RESULT_RUNTIME_MISSING;
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
    VULKANSTORY_NGX_RETURN(((pfn_init_project_id)ngx->init_project_id)(
        projectId, engineType, engineVersion, applicationDataPath,
        instance, physicalDevice, device, sdkVersion, featureCommonInfo));
}

VulkanStoryNgxResult VulkanStoryNgx_VulkanShutdown(void *device)
{
    /* Written by NGX with the SDK's remaining reference count; see pfn_shutdown1. */
    int remainingReferences = 0;
    VULKANSTORY_NGX_REQUIRE(shutdown1)
    VULKANSTORY_NGX_RETURN(((pfn_shutdown1)ngx->shutdown1)(device, &remainingReferences));
}

/* --------------------------------------------------------- parameter blocks */

VulkanStoryNgxResult VulkanStoryNgx_GetCapabilityParameters(void **outParameters)
{
    VULKANSTORY_NGX_REQUIRE(get_capability_params)
    if (outParameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outParameters = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_out_pointer)ngx->get_capability_params)(outParameters));
}

VulkanStoryNgxResult VulkanStoryNgx_AllocateParameters(void **outParameters)
{
    VULKANSTORY_NGX_REQUIRE(allocate_params)
    if (outParameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outParameters = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_out_pointer)ngx->allocate_params)(outParameters));
}

VulkanStoryNgxResult VulkanStoryNgx_DestroyParameters(void *parameters)
{
    VULKANSTORY_NGX_REQUIRE(destroy_params)
    if (parameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_handle)ngx->destroy_params)(parameters));
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
    VULKANSTORY_NGX_RETURN(((pfn_feature_requirements)ngx->feature_requirements)(
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
    VULKANSTORY_NGX_RETURN(((pfn_instance_extensions)ngx->instance_extensions)(
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
    VULKANSTORY_NGX_RETURN(((pfn_device_extensions)ngx->device_extensions)(
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
    VULKANSTORY_NGX_RETURN(((pfn_create_feature)ngx->create_feature)(
        commandBuffer, featureId, parameters, outHandle));
}

VulkanStoryNgxResult VulkanStoryNgx_CreateFeature1(
    void *device, void *commandBuffer, int featureId, void *parameters, void **outHandle)
{
    VULKANSTORY_NGX_REQUIRE(create_feature1)
    if (parameters == NULL || outHandle == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    *outHandle = NULL;
    VULKANSTORY_NGX_RETURN(((pfn_create_feature1)ngx->create_feature1)(
        device, commandBuffer, featureId, parameters, outHandle));
}

VulkanStoryNgxResult VulkanStoryNgx_EvaluateFeature(
    void *commandBuffer, void *handle, void *parameters, void *progressCallback)
{
    VULKANSTORY_NGX_REQUIRE(evaluate_feature)
    if (handle == NULL || parameters == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_evaluate_feature)ngx->evaluate_feature)(
        commandBuffer, handle, parameters, progressCallback));
}

VulkanStoryNgxResult VulkanStoryNgx_ReleaseFeature(void *handle)
{
    VULKANSTORY_NGX_REQUIRE(release_feature)
    if (handle == NULL) return VULKANSTORY_NGX_RESULT_INVALID_ARGUMENT;
    VULKANSTORY_NGX_RETURN(((pfn_handle)ngx->release_feature)(handle));
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
