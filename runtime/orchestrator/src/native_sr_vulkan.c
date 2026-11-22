/* SPDX-License-Identifier: MIT */
/* CMake-selected Vulkan SR platform and DLL export thunks. Rust (vulkan_sr.rs) owns provider
   admission, SDK descriptor assembly, image ownership and retirement. This leaf keeps only what
   Rust cannot express: the FidelityFX record layouts checked against the configured SDK headers,
   and the SDK and device-proc calls that must contain foreign C++ exceptions. MSVC raises a C++
   exception as SEH code 0xE06D7363, so the filter stops exactly what the former catch-all
   stopped; access violations and other faults still propagate. */
#include <rescaleframe/native_sr_vulkan.h>
#include "vulkan_fidelityfx_native.h"

typedef struct rsf_srvk_platform rsf_srvk_platform;
typedef int32_t (*rsf_srvk_lookup)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*);

int32_t rsf_core_srvk_create(const rsf_srvk_platform*, const rsf_srvk_setup*, rsf_srvk**);
int32_t rsf_core_srvk_plan(const rsf_srvk*, uint32_t*, uint32_t*);
int32_t rsf_core_srvk_evaluate(rsf_srvk*, void*, const rsf_srvk_frame*);
void rsf_core_srvk_retire_commands(rsf_srvk*, void*);
int32_t rsf_core_srvk_destroy(rsf_srvk*);
int32_t rsf_core_srvk_dlss_preset(rsf_srvk*, uint32_t);
int32_t rsf_core_srvk_spatial_fallback(rsf_srvk_lookup, const rsf_srvk_fallback*);

#if defined(RSF_HAVE_FFX_VK)
#include "native_dlss_vulkan_platform.h"
#include "native_xess_vulkan.h"
#include "vulkan_fg_copy_abi.h"
#include "vulkan_fg_resources.h"
#include "../../backends/fsr/src/ffx_version.h"
#include <ffx_api/ffx_api.h>
#include <ffx_api/ffx_upscale.h>
#include <stdbool.h>
/* ffx_api_vk.h names these structs without the struct keyword, as C++ allows. */
typedef struct FfxApiResource FfxApiResource;
typedef struct FfxApiResourceDescription FfxApiResourceDescription;
typedef struct VkQueueInfoFFXAPI VkQueueInfoFFXAPI;
#include <ffx_api/vk/ffx_api_vk.h>
#include <rescaleframe/input_dump.h>
#include <stddef.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL
#define RSF_CXX_FILTER (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER \
                                                                     : EXCEPTION_CONTINUE_SEARCH)

typedef struct rsf_srvk_ffx_dispatch rsf_srvk_ffx_dispatch;
typedef struct rsf_srvk_ffx_native rsf_srvk_ffx_native;
typedef rsf_vk_function rsf_srvk_ffx_exports[4];
typedef rsf_vk_function rsf_srvk_ffx_device_entries[9];
typedef struct ffxQueryDescUpscaleGetRenderResolutionFromQualityMode rsf_srvk_plan_desc;
typedef struct ffxDispatchDescUpscale rsf_srvk_dispatch_desc;

/* Field order and size are mirrored by FfxOps in vulkan_sr.rs. */
typedef struct rsf_srvk_ffx_ops {
    rsf_vk_sdk_loader loader;
    rsf_srvk_ffx_native* (*allocate)(const rsf_srvk_setup*);
    void (*close)(rsf_srvk_ffx_native*);
    rsf_vk_function (*symbol)(void*, const char*);
    void (*bind_sdk)(rsf_srvk_ffx_native*, const rsf_srvk_ffx_exports*);
    int32_t (*lookup)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*);
    int32_t (*initialize_graphics)(rsf_srvk_ffx_native*, const rsf_srvk_setup*,
        const rsf_srvk_ffx_device_entries*, rsf_vk_function);
    int32_t (*versions)(rsf_srvk_ffx_native*, rsf_ffx_version*, uint64_t*);
    int32_t (*create_effect)(rsf_srvk_ffx_native*, const rsf_srvk_setup*, uint64_t, uint32_t);
    int32_t (*effective_version)(rsf_srvk_ffx_native*, uint64_t*);
    int32_t (*destroy_effect)(rsf_srvk_ffx_native*);
    int32_t (*plan)(rsf_srvk_ffx_native*, const rsf_srvk_setup*, uint32_t*, uint32_t*);
    /* The Rust image owner sequences allocation and binding. */
    void* (*image_user)(rsf_srvk_ffx_native*);
    const rsf_vkfg_platform* images;
    uint32_t (*memory_type_count)(rsf_srvk_ffx_native*);
    void (*barrier)(rsf_srvk_ffx_native*, void*, const struct rsf_vkfg_barrier*);
    void (*clear_color_zero)(rsf_srvk_ffx_native*, void*, uint64_t, uint32_t);
    int32_t (*dispatch)(rsf_srvk_ffx_native*, void*, const rsf_srvk_ffx_dispatch*);
    void (*copy)(rsf_srvk_ffx_native*, void*, const struct rsf_vkfg_image_copy*);
    int32_t (*sharpen)(rsf_srvk_ffx_native*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*, uint64_t);
    void (*clear_sharpen)(rsf_srvk_ffx_native*);
    void (*capture_provider)(rsf_srvk_ffx_native*, uint64_t, uint32_t);
    void (*capture_record)(rsf_srvk_ffx_native*, void*, const rsf_srvk_image*, const rsf_frame_record*,
        const char*, uint32_t);
    int32_t (*capture_wants)(rsf_srvk_ffx_native*, uint64_t);
    void (*capture_parameters)(const rsf_srvk_frame*, uint32_t, uint32_t);
    void (*retire_commands)(rsf_srvk_ffx_native*, void*);
    void (*flush_capture)(rsf_srvk_ffx_native*);
} rsf_srvk_ffx_ops;

/* Field order and size are mirrored by Platform in vulkan_sr.rs. */
struct rsf_srvk_platform {
    const rsf_vkdlss_platform* dlss;
    const rsf_xessvk_ops* xess;
    rsf_srvk_ffx_ops ffx;
    int32_t (*dump_begin)(const rsf_frame_record*, uint32_t);
    int32_t (*dump_wants)(const rsf_frame_record*);
    void (*dump_result)(const rsf_frame_record*, const char*, uint32_t, int32_t);
};

#define RSF_ASSERT(condition) _Static_assert(condition, #condition)
RSF_ASSERT(sizeof(rsf_srvk_ffx_ops) == 28 * sizeof(void*) && sizeof(rsf_srvk_platform) == 264);
RSF_ASSERT(sizeof(rsf_srvk_ffx_exports) == 4 * sizeof(void*) &&
    sizeof(rsf_srvk_ffx_device_entries) == 9 * sizeof(void*));
RSF_ASSERT(offsetof(rsf_srvk_ffx_ops, barrier) == 16 * 8 && offsetof(rsf_srvk_ffx_ops, clear_color_zero) == 17 * 8 &&
    offsetof(rsf_srvk_ffx_ops, dispatch) == 18 * 8 && offsetof(rsf_srvk_ffx_ops, copy) == 19 * 8 &&
    offsetof(rsf_srvk_ffx_ops, sharpen) == 20 * 8);
RSF_ASSERT(sizeof(rsf_vk_sdk_loader) == 2 * sizeof(void*));
RSF_ASSERT(sizeof(rsf_ffx_version) == 32 && offsetof(rsf_ffx_version, major) == 16);
RSF_ASSERT(sizeof(struct rsf_vkfg_barrier) == 40 && sizeof(struct rsf_vkfg_image_copy) == 32);

/* Keep the configured SDK compatible with the pinned Rust Vulkan records. */
#define RSF_FFX_FIELD(type, field, offset) RSF_ASSERT(offsetof(type, field) == offset)
RSF_ASSERT(sizeof(ffxApiHeader) == 16 && _Alignof(ffxApiHeader) == 8);
RSF_FFX_FIELD(ffxApiHeader, type, 0);
RSF_FFX_FIELD(ffxApiHeader, pNext, 8);
RSF_ASSERT(sizeof(struct FfxApiDimensions2D) == 8 && sizeof(struct FfxApiFloatCoords2D) == 8);
RSF_FFX_FIELD(struct FfxApiDimensions2D, width, 0);
RSF_FFX_FIELD(struct FfxApiDimensions2D, height, 4);
RSF_FFX_FIELD(struct FfxApiFloatCoords2D, x, 0);
RSF_FFX_FIELD(struct FfxApiFloatCoords2D, y, 4);
RSF_ASSERT(sizeof(struct ffxCreateBackendVKDesc) == 40 && _Alignof(struct ffxCreateBackendVKDesc) == 8);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, header, 0);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, vkDevice, 16);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, vkPhysicalDevice, 24);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, vkDeviceProcAddr, 32);
RSF_ASSERT(sizeof(struct ffxCreateContextDescUpscale) == 48 && _Alignof(struct ffxCreateContextDescUpscale) == 8);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, header, 0);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, flags, 16);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, maxRenderSize, 20);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, maxUpscaleSize, 28);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, fpMessage, 40);
RSF_ASSERT(sizeof(rsf_srvk_plan_desc) == 48 && _Alignof(rsf_srvk_plan_desc) == 8);
RSF_FFX_FIELD(rsf_srvk_plan_desc, header, 0);
RSF_FFX_FIELD(rsf_srvk_plan_desc, displayWidth, 16);
RSF_FFX_FIELD(rsf_srvk_plan_desc, displayHeight, 20);
RSF_FFX_FIELD(rsf_srvk_plan_desc, qualityMode, 24);
RSF_FFX_FIELD(rsf_srvk_plan_desc, pOutRenderWidth, 32);
RSF_FFX_FIELD(rsf_srvk_plan_desc, pOutRenderHeight, 40);
RSF_ASSERT(sizeof(struct FfxApiResourceDescription) == 32 && _Alignof(struct FfxApiResourceDescription) == 4);
RSF_FFX_FIELD(struct FfxApiResourceDescription, type, 0);
RSF_FFX_FIELD(struct FfxApiResourceDescription, format, 4);
RSF_FFX_FIELD(struct FfxApiResourceDescription, width, 8);
RSF_FFX_FIELD(struct FfxApiResourceDescription, height, 12);
RSF_FFX_FIELD(struct FfxApiResourceDescription, depth, 16);
RSF_FFX_FIELD(struct FfxApiResourceDescription, mipCount, 20);
RSF_FFX_FIELD(struct FfxApiResourceDescription, flags, 24);
RSF_FFX_FIELD(struct FfxApiResourceDescription, usage, 28);
RSF_ASSERT(sizeof(struct FfxApiResource) == 48 && _Alignof(struct FfxApiResource) == 8);
RSF_FFX_FIELD(struct FfxApiResource, resource, 0);
RSF_FFX_FIELD(struct FfxApiResource, description, 8);
RSF_FFX_FIELD(struct FfxApiResource, state, 40);
RSF_ASSERT(sizeof(rsf_srvk_dispatch_desc) == 432 && _Alignof(rsf_srvk_dispatch_desc) == 8);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, header, 0);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, commandList, 16);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, color, 24);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, depth, 72);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, motionVectors, 120);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, exposure, 168);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, reactive, 216);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, transparencyAndComposition, 264);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, output, 312);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, jitterOffset, 360);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, motionVectorScale, 368);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, renderSize, 376);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, upscaleSize, 384);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, enableSharpening, 392);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, sharpness, 396);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, frameTimeDelta, 400);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, preExposure, 404);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, reset, 408);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, cameraNear, 412);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, cameraFar, 416);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, cameraFovAngleVertical, 420);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, viewSpaceToMetersFactor, 424);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, flags, 428);
RSF_ASSERT(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK == 3 &&
    FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE == 65536 &&
    FFX_API_DISPATCH_DESC_TYPE_UPSCALE == 65537 &&
    FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE == 65539);
RSF_ASSERT(FFX_API_SURFACE_FORMAT_UNKNOWN == 0 && FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT == 4 &&
    FFX_API_SURFACE_FORMAT_R16G16_FLOAT == 18 && FFX_API_SURFACE_FORMAT_R8_UNORM == 25 &&
    FFX_API_SURFACE_FORMAT_R32_FLOAT == 28);
RSF_ASSERT(FFX_API_RESOURCE_TYPE_TEXTURE2D == 2 && FFX_API_RESOURCE_USAGE_READ_ONLY == 0 &&
    FFX_API_RESOURCE_USAGE_UAV == 2 && FFX_API_RESOURCE_USAGE_DEPTHTARGET == 4 &&
    FFX_API_RESOURCE_STATE_UNORDERED_ACCESS == 2 && FFX_API_RESOURCE_STATE_COMPUTE_READ == 4);
RSF_ASSERT(FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE == 1 && FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION == 4 &&
    FFX_UPSCALE_ENABLE_DEPTH_INVERTED == 8 && FFX_UPSCALE_ENABLE_DEPTH_INFINITE == 16 &&
    FFX_UPSCALE_ENABLE_AUTO_EXPOSURE == 32);
RSF_ASSERT(RSF_SHARPENING_VENDOR == 4);

void rsf_core_srvk_ffx_create_desc(const rsf_srvk_setup*, uint32_t, ffxApiHeader*,
    struct ffxCreateBackendVKDesc*, struct ffxCreateContextDescUpscale*);
void rsf_core_srvk_ffx_plan_desc(const rsf_srvk_setup*, uint32_t*, uint32_t*, rsf_srvk_plan_desc*);
void rsf_core_srvk_ffx_dispatch_desc(void*, const rsf_srvk_ffx_dispatch*, rsf_srvk_dispatch_desc*);

static int32_t call_device_proc(rsf_vk_get_proc proc, void* device, const char* name, rsf_vk_function* result)
{
    __try {
        *result = proc(device, name);
        return 1;
    } __except (RSF_CXX_FILTER) {
        return 0;
    }
}

/* The SDK record the Rust owner keeps for this leaf. */
typedef struct rsf_srvk_sdk {
    rsf_vkffx_exports exports;
    ffxContext effect;
} rsf_srvk_sdk;
RSF_ASSERT(sizeof(rsf_srvk_sdk) == 40 && _Alignof(rsf_srvk_sdk) == 8 && offsetof(rsf_srvk_sdk, effect) == 32);
RSF_ASSERT(sizeof(rsf_vkffx_exports) == 32 && offsetof(rsf_vkffx_exports, create) == 0 &&
    offsetof(rsf_vkffx_exports, destroy) == 8 && offsetof(rsf_vkffx_exports, query) == 16 &&
    offsetof(rsf_vkffx_exports, dispatch) == 24);

rsf_srvk_ffx_native* rsf_core_srvk_native_allocate(const rsf_srvk_setup*);
void rsf_core_srvk_native_close(rsf_srvk_ffx_native*);
void rsf_core_srvk_native_bind(rsf_srvk_ffx_native*, const rsf_srvk_ffx_exports*);
rsf_srvk_sdk* rsf_core_srvk_native_sdk(rsf_srvk_ffx_native*);
int32_t rsf_core_srvk_graphics_initialize(rsf_srvk_ffx_native*, const rsf_srvk_setup*,
    const rsf_srvk_ffx_device_entries*, rsf_vk_function);
void* rsf_core_srvk_graphics_images(rsf_srvk_ffx_native*);
uint32_t rsf_core_srvk_graphics_memory_count(rsf_srvk_ffx_native*);
void rsf_core_srvk_graphics_barrier(rsf_srvk_ffx_native*, void*, const struct rsf_vkfg_barrier*);
void rsf_core_srvk_graphics_clear(rsf_srvk_ffx_native*, void*, uint64_t, uint32_t);
void rsf_core_srvk_graphics_copy(rsf_srvk_ffx_native*, void*, const struct rsf_vkfg_image_copy*);
int32_t rsf_core_srvk_graphics_sharpen(rsf_srvk_ffx_native*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*,
    uint64_t);
void rsf_core_srvk_graphics_clear_sharpen(rsf_srvk_ffx_native*);
void rsf_core_srvk_graphics_capture_provider(rsf_srvk_ffx_native*, uint64_t, uint32_t);
void rsf_core_srvk_graphics_capture_record(rsf_srvk_ffx_native*, void*, const rsf_srvk_image*,
    const rsf_frame_record*, const char*, uint32_t);
int32_t rsf_core_srvk_graphics_capture_wants(rsf_srvk_ffx_native*, uint64_t);
void rsf_core_srvk_graphics_capture_parameters(const rsf_srvk_frame*, uint32_t, uint32_t);
void rsf_core_srvk_graphics_retire(rsf_srvk_ffx_native*, void*);
void rsf_core_srvk_graphics_flush(rsf_srvk_ffx_native*);

static int32_t versions(rsf_srvk_ffx_native* native, rsf_ffx_version* out, uint64_t* count)
{
    __try {
        rsf_srvk_sdk* sdk = rsf_core_srvk_native_sdk(native);
        uint64_t ids[RSF_FFX_MAX_VERSIONS] = {0};
        const char* names[RSF_FFX_MAX_VERSIONS] = {0};
        struct ffxQueryDescGetVersions query = {0};
        *count = RSF_FFX_MAX_VERSIONS;
        query.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
        query.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
        query.outputCount = count;
        query.versionIds = ids;
        query.versionNames = names;
        if (sdk->exports.query(NULL, &query.header) != FFX_API_RETURN_OK || *count > RSF_FFX_MAX_VERSIONS)
            return RSF_BACKEND_ERROR_NOT_SUPPORTED;
        for (uint64_t index = 0; index < *count; ++index)
            (void)rsf_fsr4_parse_version(ids[index], names[index], &out[index]);
        return RSF_BACKEND_OK;
    } __except (RSF_CXX_FILTER) {
        return RSF_BACKEND_ERROR_INIT_FAILED;
    }
}

static int32_t create_effect(rsf_srvk_ffx_native* native, const rsf_srvk_setup* setup, uint64_t id, uint32_t flags)
{
    __try {
        rsf_srvk_sdk* sdk = rsf_core_srvk_native_sdk(native);
        struct ffxOverrideVersion version = {0};
        struct ffxCreateBackendVKDesc backend = {0};
        struct ffxCreateContextDescUpscale create = {0};
        version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
        version.versionId = id;
        rsf_core_srvk_ffx_create_desc(setup, flags, &version.header, &backend, &create);
        return sdk->exports.create(&sdk->effect, &create.header, NULL) == FFX_API_RETURN_OK
            ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
    } __except (RSF_CXX_FILTER) {
        return RSF_BACKEND_ERROR_INIT_FAILED;
    }
}

static int32_t effective_version(rsf_srvk_ffx_native* native, uint64_t* out)
{
    __try {
        rsf_srvk_sdk* sdk = rsf_core_srvk_native_sdk(native);
        struct ffxQueryGetProviderVersion query = {0};
        query.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
        if (sdk->exports.query(&sdk->effect, &query.header) != FFX_API_RETURN_OK)
            return RSF_BACKEND_ERROR_NOT_SUPPORTED;
        *out = query.versionId;
        return RSF_BACKEND_OK;
    } __except (RSF_CXX_FILTER) {
        return RSF_BACKEND_ERROR_INIT_FAILED;
    }
}

static int32_t destroy_effect(rsf_srvk_ffx_native* native)
{
    __try {
        rsf_srvk_sdk* sdk = rsf_core_srvk_native_sdk(native);
        if (!sdk->effect) return RSF_BACKEND_OK;
        if (sdk->exports.destroy(&sdk->effect, NULL) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        sdk->effect = NULL;
        return RSF_BACKEND_OK;
    } __except (RSF_CXX_FILTER) {
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
}

static int32_t plan(rsf_srvk_ffx_native* native, const rsf_srvk_setup* setup, uint32_t* width, uint32_t* height)
{
    __try {
        rsf_srvk_sdk* sdk = rsf_core_srvk_native_sdk(native);
        rsf_srvk_plan_desc query = {0};
        rsf_core_srvk_ffx_plan_desc(setup, width, height, &query);
        return sdk->exports.query(&sdk->effect, &query.header) == FFX_API_RETURN_OK
            ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_SUPPORTED;
    } __except (RSF_CXX_FILTER) {
        return RSF_BACKEND_ERROR_INIT_FAILED;
    }
}

static int32_t dispatch(rsf_srvk_ffx_native* native, void* commands, const rsf_srvk_ffx_dispatch* call)
{
    __try {
        rsf_srvk_sdk* sdk = rsf_core_srvk_native_sdk(native);
        rsf_srvk_dispatch_desc data = {0};
        rsf_core_srvk_ffx_dispatch_desc(commands, call, &data);
        return sdk->exports.dispatch(&sdk->effect, &data.header) == FFX_API_RETURN_OK
            ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    } __except (RSF_CXX_FILTER) {
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
}

static const rsf_srvk_platform native = {
    RSF_VKDLSS_NATIVE_PLATFORM,
    RSF_XESSVK_NATIVE_PLATFORM,
    {RSF_VK_SDK_SYSTEM_LOADER, rsf_core_srvk_native_allocate, rsf_core_srvk_native_close,
        rsf_vkffx_symbol, rsf_core_srvk_native_bind, call_device_proc, rsf_core_srvk_graphics_initialize,
        versions, create_effect, effective_version, destroy_effect, plan,
        rsf_core_srvk_graphics_images, &rsf_core_vkfg_platform, rsf_core_srvk_graphics_memory_count,
        rsf_core_srvk_graphics_barrier, rsf_core_srvk_graphics_clear, dispatch,
        rsf_core_srvk_graphics_copy, rsf_core_srvk_graphics_sharpen, rsf_core_srvk_graphics_clear_sharpen,
        rsf_core_srvk_graphics_capture_provider, rsf_core_srvk_graphics_capture_record,
        rsf_core_srvk_graphics_capture_wants, rsf_core_srvk_graphics_capture_parameters,
        rsf_core_srvk_graphics_retire, rsf_core_srvk_graphics_flush},
    rsf_input_dump_begin_record, rsf_input_dump_wants, rsf_input_dump_result,
};
#define RSF_SRVK_PLATFORM (&native)
#define RSF_SRVK_FALLBACK_LOOKUP call_device_proc
#else
/* Without the FidelityFX Vulkan headers, valid creation returns NOT_COMPILED and valid
   spatial recovery returns NOT_SUPPORTED; vulkan_sr.rs owns both decisions. */
#define RSF_SRVK_PLATFORM ((const rsf_srvk_platform*)0)
#define RSF_SRVK_FALLBACK_LOOKUP ((rsf_srvk_lookup)0)
#endif

rsf_backend_result rsf_srvk_create(const rsf_srvk_setup* setup, rsf_srvk** out)
{
    return rsf_core_srvk_create(RSF_SRVK_PLATFORM, setup, out);
}

rsf_backend_result rsf_srvk_plan(const rsf_srvk* owner, uint32_t* width, uint32_t* height)
{
    return rsf_core_srvk_plan(owner, width, height);
}

rsf_backend_result rsf_srvk_evaluate(rsf_srvk* owner, void* commands, const rsf_srvk_frame* frame)
{
    return rsf_core_srvk_evaluate(owner, commands, frame);
}

void rsf_srvk_retire_commands(rsf_srvk* owner, void* commands)
{
    rsf_core_srvk_retire_commands(owner, commands);
}

rsf_backend_result rsf_srvk_destroy(rsf_srvk* owner)
{
    return rsf_core_srvk_destroy(owner);
}

rsf_backend_result rsf_srvk_dlss_preset(rsf_srvk* owner, uint32_t preset)
{
    return rsf_core_srvk_dlss_preset(owner, preset);
}

rsf_backend_result rsf_srvk_spatial_fallback(const rsf_srvk_fallback* call)
{
    return rsf_core_srvk_spatial_fallback(RSF_SRVK_FALLBACK_LOOKUP, call);
}
