// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr_vulkan.h>
#include "vulkan_fidelityfx_native.h"

namespace {
struct Platform;
using Lookup = int32_t (*)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*);
}

extern "C" {
int32_t rsf_core_srvk_create(const Platform*, const rsf_srvk_setup*, rsf_srvk**);
int32_t rsf_core_srvk_plan(const rsf_srvk*, uint32_t*, uint32_t*);
int32_t rsf_core_srvk_evaluate(rsf_srvk*, void*, const rsf_srvk_frame*);
void rsf_core_srvk_retire_commands(rsf_srvk*, void*);
int32_t rsf_core_srvk_destroy(rsf_srvk*);
int32_t rsf_core_srvk_dlss_preset(rsf_srvk*, uint32_t);
int32_t rsf_core_srvk_spatial_fallback(Lookup, const rsf_srvk_fallback*);
}

#if defined(RSF_HAVE_FFX_VK)
#include "native_dlss_vulkan_platform.h"
#include "native_xess_vulkan.h"
#include "vulkan_fg_copy_abi.h"
#include "vulkan_fg_resources.h"
#include "../../backends/fsr/src/ffx_version.h"
#include <ffx_api/ffx_api.h>
#include <ffx_api/ffx_upscale.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include <rescaleframe/input_dump.h>

extern "C" const rsf_vkfg_platform rsf_core_vkfg_platform;

namespace {
constexpr auto max_versions = RSF_FFX_MAX_VERSIONS;

using Version = rsf_ffx_version;
struct FfxDispatch;
struct FfxNative;
using FfxExports = rsf_vk_function[4];
using FfxDeviceEntries = rsf_vk_function[9];
struct FfxOps {
    rsf_vk_sdk_loader loader;
    FfxNative* (*allocate)(const rsf_srvk_setup*);
    void (*close)(FfxNative*);
    rsf_vk_function (*symbol)(void*, const char*);
    void (*bind_sdk)(FfxNative*, const FfxExports*);
    int32_t (*lookup)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*);
    int32_t (*initialize_graphics)(FfxNative*, const rsf_srvk_setup*, const FfxDeviceEntries*, rsf_vk_function);
    int32_t (*versions)(FfxNative*, Version*, uint64_t*);
    int32_t (*create_effect)(FfxNative*, const rsf_srvk_setup*, uint64_t, uint32_t);
    int32_t (*effective_version)(FfxNative*, uint64_t*);
    int32_t (*destroy_effect)(FfxNative*);
    int32_t (*plan)(FfxNative*, const rsf_srvk_setup*, uint32_t*, uint32_t*);
    // The Rust image owner sequences allocation and binding.
    void* (*image_user)(FfxNative*);
    const rsf_vkfg_platform* images;
    uint32_t (*memory_type_count)(FfxNative*);
    void (*barrier)(FfxNative*, void*, const rsf_vkfg_barrier*);
    void (*clear_color_zero)(FfxNative*, void*, uint64_t, uint32_t);
    int32_t (*dispatch)(FfxNative*, void*, const FfxDispatch*);
    void (*copy)(FfxNative*, void*, const rsf_vkfg_image_copy*);
    int32_t (*sharpen)(FfxNative*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*, uint64_t);
    void (*clear_sharpen)(FfxNative*);
    void (*capture_provider)(FfxNative*, uint64_t, uint32_t);
    void (*capture_record)(FfxNative*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint32_t);
    int32_t (*capture_wants)(FfxNative*, uint64_t);
    void (*capture_parameters)(const rsf_srvk_frame*, uint32_t, uint32_t);
    void (*retire_commands)(FfxNative*, void*);
    void (*flush_capture)(FfxNative*);
};
struct Platform {
    const rsf_vkdlss_platform* dlss;
    const rsf::xess_vk::Ops* xess;
    FfxOps ffx;
    int32_t (*dump_begin)(const rsf_frame_record*, uint32_t);
    int32_t (*dump_wants)(const rsf_frame_record*);
    void (*dump_result)(const rsf_frame_record*, const char*, uint32_t, int32_t);
};
static_assert(sizeof(FfxOps) == 28 * sizeof(void*) && sizeof(Platform) == 264);
static_assert(sizeof(FfxExports) == 4 * sizeof(void*) && sizeof(FfxDeviceEntries) == 9 * sizeof(void*));
static_assert(offsetof(FfxOps, barrier) == 16 * 8 && offsetof(FfxOps, clear_color_zero) == 17 * 8 &&
    offsetof(FfxOps, dispatch) == 18 * 8 && offsetof(FfxOps, copy) == 19 * 8 && offsetof(FfxOps, sharpen) == 20 * 8);
// Keep the configured SDK compatible with the pinned Rust Vulkan records.
#define RSF_FFX_FIELD(type, field, offset) static_assert(offsetof(type, field) == offset)
static_assert(sizeof(ffxApiHeader) == 16 && alignof(ffxApiHeader) == 8);
RSF_FFX_FIELD(ffxApiHeader, type, 0);
RSF_FFX_FIELD(ffxApiHeader, pNext, 8);
static_assert(sizeof(FfxApiDimensions2D) == 8 && sizeof(FfxApiFloatCoords2D) == 8);
RSF_FFX_FIELD(FfxApiDimensions2D, width, 0);
RSF_FFX_FIELD(FfxApiDimensions2D, height, 4);
RSF_FFX_FIELD(FfxApiFloatCoords2D, x, 0);
RSF_FFX_FIELD(FfxApiFloatCoords2D, y, 4);
static_assert(sizeof(ffxCreateBackendVKDesc) == 40 && alignof(ffxCreateBackendVKDesc) == 8);
RSF_FFX_FIELD(ffxCreateBackendVKDesc, header, 0);
RSF_FFX_FIELD(ffxCreateBackendVKDesc, vkDevice, 16);
RSF_FFX_FIELD(ffxCreateBackendVKDesc, vkPhysicalDevice, 24);
RSF_FFX_FIELD(ffxCreateBackendVKDesc, vkDeviceProcAddr, 32);
static_assert(sizeof(ffxCreateContextDescUpscale) == 48 && alignof(ffxCreateContextDescUpscale) == 8);
RSF_FFX_FIELD(ffxCreateContextDescUpscale, header, 0);
RSF_FFX_FIELD(ffxCreateContextDescUpscale, flags, 16);
RSF_FFX_FIELD(ffxCreateContextDescUpscale, maxRenderSize, 20);
RSF_FFX_FIELD(ffxCreateContextDescUpscale, maxUpscaleSize, 28);
RSF_FFX_FIELD(ffxCreateContextDescUpscale, fpMessage, 40);
using PlanDesc = ffxQueryDescUpscaleGetRenderResolutionFromQualityMode;
static_assert(sizeof(PlanDesc) == 48 && alignof(PlanDesc) == 8);
RSF_FFX_FIELD(PlanDesc, header, 0);
RSF_FFX_FIELD(PlanDesc, displayWidth, 16);
RSF_FFX_FIELD(PlanDesc, displayHeight, 20);
RSF_FFX_FIELD(PlanDesc, qualityMode, 24);
RSF_FFX_FIELD(PlanDesc, pOutRenderWidth, 32);
RSF_FFX_FIELD(PlanDesc, pOutRenderHeight, 40);
static_assert(sizeof(FfxApiResourceDescription) == 32 && alignof(FfxApiResourceDescription) == 4);
RSF_FFX_FIELD(FfxApiResourceDescription, type, 0);
RSF_FFX_FIELD(FfxApiResourceDescription, format, 4);
RSF_FFX_FIELD(FfxApiResourceDescription, width, 8);
RSF_FFX_FIELD(FfxApiResourceDescription, height, 12);
RSF_FFX_FIELD(FfxApiResourceDescription, depth, 16);
RSF_FFX_FIELD(FfxApiResourceDescription, mipCount, 20);
RSF_FFX_FIELD(FfxApiResourceDescription, flags, 24);
RSF_FFX_FIELD(FfxApiResourceDescription, usage, 28);
static_assert(sizeof(FfxApiResource) == 48 && alignof(FfxApiResource) == 8);
RSF_FFX_FIELD(FfxApiResource, resource, 0);
RSF_FFX_FIELD(FfxApiResource, description, 8);
RSF_FFX_FIELD(FfxApiResource, state, 40);
using DispatchDesc = ffxDispatchDescUpscale;
static_assert(sizeof(DispatchDesc) == 432 && alignof(DispatchDesc) == 8);
RSF_FFX_FIELD(DispatchDesc, header, 0);
RSF_FFX_FIELD(DispatchDesc, commandList, 16);
RSF_FFX_FIELD(DispatchDesc, color, 24);
RSF_FFX_FIELD(DispatchDesc, depth, 72);
RSF_FFX_FIELD(DispatchDesc, motionVectors, 120);
RSF_FFX_FIELD(DispatchDesc, exposure, 168);
RSF_FFX_FIELD(DispatchDesc, reactive, 216);
RSF_FFX_FIELD(DispatchDesc, transparencyAndComposition, 264);
RSF_FFX_FIELD(DispatchDesc, output, 312);
RSF_FFX_FIELD(DispatchDesc, jitterOffset, 360);
RSF_FFX_FIELD(DispatchDesc, motionVectorScale, 368);
RSF_FFX_FIELD(DispatchDesc, renderSize, 376);
RSF_FFX_FIELD(DispatchDesc, upscaleSize, 384);
RSF_FFX_FIELD(DispatchDesc, enableSharpening, 392);
RSF_FFX_FIELD(DispatchDesc, sharpness, 396);
RSF_FFX_FIELD(DispatchDesc, frameTimeDelta, 400);
RSF_FFX_FIELD(DispatchDesc, preExposure, 404);
RSF_FFX_FIELD(DispatchDesc, reset, 408);
RSF_FFX_FIELD(DispatchDesc, cameraNear, 412);
RSF_FFX_FIELD(DispatchDesc, cameraFar, 416);
RSF_FFX_FIELD(DispatchDesc, cameraFovAngleVertical, 420);
RSF_FFX_FIELD(DispatchDesc, viewSpaceToMetersFactor, 424);
RSF_FFX_FIELD(DispatchDesc, flags, 428);
#undef RSF_FFX_FIELD
static_assert(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK == 3 &&
    FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE == 65536 &&
    FFX_API_DISPATCH_DESC_TYPE_UPSCALE == 65537 &&
    FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE == 65539);
static_assert(FFX_API_SURFACE_FORMAT_UNKNOWN == 0 && FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT == 4 &&
    FFX_API_SURFACE_FORMAT_R16G16_FLOAT == 18 && FFX_API_SURFACE_FORMAT_R8_UNORM == 25 &&
    FFX_API_SURFACE_FORMAT_R32_FLOAT == 28);
static_assert(FFX_API_RESOURCE_TYPE_TEXTURE2D == 2 && FFX_API_RESOURCE_USAGE_READ_ONLY == 0 &&
    FFX_API_RESOURCE_USAGE_UAV == 2 && FFX_API_RESOURCE_USAGE_DEPTHTARGET == 4 &&
    FFX_API_RESOURCE_STATE_UNORDERED_ACCESS == 2 && FFX_API_RESOURCE_STATE_COMPUTE_READ == 4);

extern "C" {
void rsf_core_srvk_ffx_create_desc(const rsf_srvk_setup*, uint32_t, ffxApiHeader*,
    ffxCreateBackendVKDesc*, ffxCreateContextDescUpscale*);
void rsf_core_srvk_ffx_plan_desc(const rsf_srvk_setup*, uint32_t*, uint32_t*, PlanDesc*);
void rsf_core_srvk_ffx_dispatch_desc(void*, const FfxDispatch*, DispatchDesc*);
}
static_assert(VK_FORMAT_R8_UNORM == 9 && VK_FORMAT_R16G16_SFLOAT == 83 &&
    VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_D32_SFLOAT == 126);
static_assert(VK_IMAGE_LAYOUT_GENERAL == 1 && VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 &&
    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7);
static_assert(VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_IMAGE_USAGE_TRANSFER_DST_BIT == 2 &&
    VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8);
static_assert(VK_ACCESS_SHADER_READ_BIT == 0x20 && VK_ACCESS_SHADER_WRITE_BIT == 0x40 &&
    VK_ACCESS_TRANSFER_READ_BIT == 0x800 && VK_ACCESS_TRANSFER_WRITE_BIT == 0x1000);
static_assert((VK_FORMAT_FEATURE_BLIT_SRC_BIT | VK_FORMAT_FEATURE_BLIT_DST_BIT |
    VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT) == (0x400 | 0x800 | 0x1000));
static_assert(FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE == 1 && FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION == 4 &&
    FFX_UPSCALE_ENABLE_DEPTH_INVERTED == 8 && FFX_UPSCALE_ENABLE_DEPTH_INFINITE == 16 && FFX_UPSCALE_ENABLE_AUTO_EXPOSURE == 32);
static_assert(RSF_SHARPENING_VENDOR == 4);
// rsf::vkfg::platform leaves sharingMode and initialLayout zero; the former FFX leaf requested the same values.
static_assert(VK_SHARING_MODE_EXCLUSIVE == 0 && VK_IMAGE_LAYOUT_UNDEFINED == 0);

int32_t call_device_proc(rsf_vk_get_proc proc, void* device, const char* name, rsf_vk_function* result) noexcept try
{
    *result = proc(device, name);
    return 1;
}
catch (...) { return 0; }

struct Sdk {
    rsf_vkffx_exports exports;
    ffxContext effect;
};
static_assert(sizeof(Sdk) == 40 && alignof(Sdk) == 8 && offsetof(Sdk, effect) == 32);
static_assert(sizeof(rsf_vkffx_exports) == 32 && offsetof(rsf_vkffx_exports, create) == 0 &&
    offsetof(rsf_vkffx_exports, destroy) == 8 && offsetof(rsf_vkffx_exports, query) == 16 &&
    offsetof(rsf_vkffx_exports, dispatch) == 24);

extern "C" {
FfxNative* rsf_core_srvk_native_allocate(const rsf_srvk_setup*);
void rsf_core_srvk_native_close(FfxNative*);
void rsf_core_srvk_native_bind(FfxNative*, const FfxExports*);
Sdk* rsf_core_srvk_native_sdk(FfxNative*);
int32_t rsf_core_srvk_graphics_initialize(FfxNative*, const rsf_srvk_setup*, const FfxDeviceEntries*, rsf_vk_function);
void* rsf_core_srvk_graphics_images(FfxNative*);
uint32_t rsf_core_srvk_graphics_memory_count(FfxNative*);
void rsf_core_srvk_graphics_barrier(FfxNative*, void*, const rsf_vkfg_barrier*);
void rsf_core_srvk_graphics_clear(FfxNative*, void*, uint64_t, uint32_t);
void rsf_core_srvk_graphics_copy(FfxNative*, void*, const rsf_vkfg_image_copy*);
int32_t rsf_core_srvk_graphics_sharpen(FfxNative*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*, uint64_t);
void rsf_core_srvk_graphics_clear_sharpen(FfxNative*);
void rsf_core_srvk_graphics_capture_provider(FfxNative*, uint64_t, uint32_t);
void rsf_core_srvk_graphics_capture_record(FfxNative*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint32_t);
int32_t rsf_core_srvk_graphics_capture_wants(FfxNative*, uint64_t);
void rsf_core_srvk_graphics_capture_parameters(const rsf_srvk_frame*, uint32_t, uint32_t);
void rsf_core_srvk_graphics_retire(FfxNative*, void*);
void rsf_core_srvk_graphics_flush(FfxNative*);
}

int32_t versions(FfxNative* native, Version* out, uint64_t* count) noexcept try
{
    auto& sdk = *rsf_core_srvk_native_sdk(native);
    uint64_t ids[max_versions]{};
    const char* names[max_versions]{};
    *count = max_versions;
    ffxQueryDescGetVersions query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    query.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    query.outputCount = count;
    query.versionIds = ids;
    query.versionNames = names;
    if (sdk.exports.query(nullptr, &query.header) != FFX_API_RETURN_OK || *count > max_versions)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    for (uint64_t index = 0; index < *count; ++index) {
        (void)rsf_fsr4_parse_version(ids[index], names[index], &out[index]);
    }
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t create_effect(FfxNative* native, const rsf_srvk_setup* setup, uint64_t id, uint32_t flags) noexcept try
{
    auto& sdk = *rsf_core_srvk_native_sdk(native);
    ffxOverrideVersion version{};
    version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
    version.versionId = id;
    ffxCreateBackendVKDesc backend{};
    ffxCreateContextDescUpscale create{};
    rsf_core_srvk_ffx_create_desc(setup, flags, &version.header, &backend, &create);
    return sdk.exports.create(&sdk.effect, &create.header, nullptr) == FFX_API_RETURN_OK
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t effective_version(FfxNative* native, uint64_t* out) noexcept try
{
    auto& sdk = *rsf_core_srvk_native_sdk(native);
    ffxQueryGetProviderVersion query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (sdk.exports.query(&sdk.effect, &query.header) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    *out = query.versionId;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t destroy_effect(FfxNative* native) noexcept try
{
    auto& sdk = *rsf_core_srvk_native_sdk(native);
    if (!sdk.effect) return RSF_BACKEND_OK;
    if (sdk.exports.destroy(&sdk.effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    sdk.effect = nullptr;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t plan(FfxNative* native, const rsf_srvk_setup* setup, uint32_t* width, uint32_t* height) noexcept try
{
    auto& sdk = *rsf_core_srvk_native_sdk(native);
    PlanDesc query{};
    rsf_core_srvk_ffx_plan_desc(setup, width, height, &query);
    return sdk.exports.query(&sdk.effect, &query.header) == FFX_API_RETURN_OK
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_SUPPORTED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t dispatch(FfxNative* native, void* commands, const FfxDispatch* call) noexcept try
{
    auto& sdk = *rsf_core_srvk_native_sdk(native);
    DispatchDesc data{};
    rsf_core_srvk_ffx_dispatch_desc(commands, call, &data);
    return sdk.exports.dispatch(&sdk.effect, &data.header) == FFX_API_RETURN_OK
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

constexpr Platform native{
    rsf::dlss_vk::platform,
    rsf::xess_vk::platform,
    {rsf_vk_sdk_system_loader, rsf_core_srvk_native_allocate, rsf_core_srvk_native_close,
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
constexpr const Platform* platform = &native;
constexpr Lookup fallback_lookup = call_device_proc;
}
#else
namespace {
constexpr const Platform* platform = nullptr;
constexpr Lookup fallback_lookup = nullptr;
}
#endif

extern "C" {
rsf_backend_result rsf_srvk_create(const rsf_srvk_setup* setup, rsf_srvk** out)
{
    return rsf_core_srvk_create(platform, setup, out);
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
    return rsf_core_srvk_spatial_fallback(fallback_lookup, call);
}
}
