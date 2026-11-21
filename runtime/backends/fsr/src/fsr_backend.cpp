/* SPDX-License-Identifier: MIT */
/* FSR2, FSR3 and FSR4 reconstruction through the official versioned DX12 API.
   The legacy frame-generation vtable is retained for contract callers. */

#include <rescaleframe/backend.h>
#include <rescaleframe/fsr_backend.h>

#include "../../common/sr_helpers.h"
#if defined(RSF_HAVE_FFX)
#include "../../common/sharpening.h"
#include "../../common/sr12_platform.h"
#include "ffx_version.h"
#include "fsr4_compat.h"
#include <ffx_api.h>
#include <ffx_upscale.h>
#include <dx12/ffx_api_dx12.h>
#include <cstddef>
#include <memory>
#include <new>
#endif

namespace {

rsf_backend_result probe(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps)
{
    const auto begun = rsf::probe_begin(desc, caps, RSF_VENDOR_AMD, "FidelityFX");
    if (begun != RSF_BACKEND_OK) return begun;

#if !defined(RSF_HAVE_FFX)
    caps->available = 0;
    caps->refusal_utf8 = "built without the FidelityFX headers";
    rsf::probe_say(desc, "fsr: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    caps->supported_lifetimes = 1u << RSF_LIFETIME_ONLY_NOW;

    /* Probe reports build capability. Open verifies runtime, adapter and requested version. */
    caps->available = 1;
    rsf::probe_say(desc, "fsr: SR compiled in, D3D12 only; frame generation unavailable");
    return RSF_BACKEND_OK;
#endif
}

/* Scaffolding: the legacy rsf_fg_provider vtable. Frame generation is reached through
   rsf_generation_fsr() in fsr_generation.cpp, so these only answer for callers of the old contract
   (tests/fsr_backend.cpp). The answer is the same in both builds apart from the code. */
#if !defined(RSF_HAVE_FFX)
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_COMPILED;
#else
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_READY;
#endif

#if !defined(RSF_HAVE_FFX)

rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result sr_release(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
void sr_close(void*) {}

#else

// The Rust owner retains this storage through context destruction. The
// SDK's chained create descriptions must have stable addresses.
struct FsrBindings {
    std::shared_ptr<Fsr4Compatibility> compatibility;
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxQuery query{};
    PfnFfxDispatch dispatch{};
    ffxCreateContextDescUpscale description{};
    ffxCreateBackendDX12Desc device{};
    ffxOverrideVersion override_version{};
};

using FsrVersion = rsf_ffx_version;

constexpr auto max_versions = rsf::max_ffx_versions;
static_assert(RSF_QUALITY_NATIVE == FFX_UPSCALE_QUALITY_MODE_NATIVEAA &&
    RSF_QUALITY_QUALITY == FFX_UPSCALE_QUALITY_MODE_QUALITY &&
    RSF_QUALITY_BALANCED == FFX_UPSCALE_QUALITY_MODE_BALANCED &&
    RSF_QUALITY_PERFORMANCE == FFX_UPSCALE_QUALITY_MODE_PERFORMANCE &&
    RSF_QUALITY_ULTRA_PERFORMANCE == FFX_UPSCALE_QUALITY_MODE_ULTRA_PERFORMANCE);

void* load(const rsf_sr_open_desc* desc)
{
    return rsf::load_runtime(*desc, L"amd_fidelityfx_upscaler_dx12.dll");
}

rsf_backend_result bind(void* module, const rsf_sr_open_desc* desc, uint32_t enable_int8, void** out)
{
    auto sdk = std::unique_ptr<FsrBindings>(new (std::nothrow) FsrBindings{});
    if (!sdk) return RSF_BACKEND_ERROR_INIT_FAILED;
    const auto library = static_cast<HMODULE>(module);
    if (enable_int8)
        sdk->compatibility = rsf_fsr4_enable_int8(library, static_cast<ID3D12Device*>(desc->device),
            desc->log, desc->log_user);
    sdk->create = rsf::entry<PfnFfxCreateContext>(library, "ffxCreateContext");
    sdk->destroy = rsf::entry<PfnFfxDestroyContext>(library, "ffxDestroyContext");
    sdk->query = rsf::entry<PfnFfxQuery>(library, "ffxQuery");
    sdk->dispatch = rsf::entry<PfnFfxDispatch>(library, "ffxDispatch");
    if (!sdk->create || !sdk->destroy || !sdk->query || !sdk->dispatch)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    *out = sdk.release();
    return RSF_BACKEND_OK;
}

void unbind(void* bindings)
{
    delete static_cast<FsrBindings*>(bindings);
}

rsf_backend_result versions(void* bindings, void* device, FsrVersion* out, uint64_t* count)
{
    uint64_t ids[max_versions]{};
    const char* names[max_versions]{};
    *count = max_versions;
    ffxQueryDescGetVersions query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    query.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    query.device = device;
    query.outputCount = count;
    query.versionIds = ids;
    query.versionNames = names;
    if (static_cast<FsrBindings*>(bindings)->query(nullptr, &query.header) != FFX_API_RETURN_OK ||
        *count > max_versions)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    for (uint64_t i = 0; i < *count; ++i) {
        out[i] = rsf::ffx_version(ids[i], names[i]);
    }
    return RSF_BACKEND_OK;
}

rsf_backend_result create(void* bindings, const rsf_sr_open_desc* desc, uint64_t version, void** context)
{
    auto& sdk = *static_cast<FsrBindings*>(bindings);
    sdk.device.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    sdk.device.device = static_cast<ID3D12Device*>(desc->device);
    sdk.device.header.pNext = &sdk.override_version.header;
    sdk.override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
    sdk.override_version.versionId = version;
    auto& create = sdk.description;
    create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    create.header.pNext = &sdk.device.header;
    create.maxRenderSize = {desc->output_width, desc->output_height};
    create.maxUpscaleSize = create.maxRenderSize;
    create.flags = (desc->hdr ? FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE : 0u) |
        (desc->inverted_depth ? FFX_UPSCALE_ENABLE_DEPTH_INVERTED : 0u) |
        (desc->depth_infinite ? FFX_UPSCALE_ENABLE_DEPTH_INFINITE : 0u) |
        (desc->dynamic_resolution ? FFX_UPSCALE_ENABLE_DYNAMIC_RESOLUTION : 0u) |
        (desc->motion_jittered ? FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u) |
        (desc->auto_exposure ? FFX_UPSCALE_ENABLE_AUTO_EXPOSURE : 0u);
    return sdk.create(context, &create.header, nullptr) == FFX_API_RETURN_OK ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}

void destroy(void* bindings, void** context)
{
    static_cast<FsrBindings*>(bindings)->destroy(context, nullptr);
}

rsf_backend_result effective_version(void* bindings, void** context, uint64_t* version)
{
    ffxQueryGetProviderVersion query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (static_cast<FsrBindings*>(bindings)->query(context, &query.header) != FFX_API_RETURN_OK)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    *version = query.versionId;
    return RSF_BACKEND_OK;
}

rsf_backend_result plan(void* bindings, void** context, const uint32_t* output, rsf_quality quality,
    uint32_t* width, uint32_t* height)
{
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
    query.displayWidth = output[0];
    query.displayHeight = output[1];
    query.qualityMode = quality;
    query.pOutRenderWidth = width;
    query.pOutRenderHeight = height;
    return static_cast<FsrBindings*>(bindings)->query(context, &query.header) == FFX_API_RETURN_OK ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

rsf_backend_result dispatch(void* bindings, void** context, void* commands, const rsf_sr_frame* frame,
    float far_plane)
{
    const auto& record = *frame->record;
    ffxDispatchDescUpscale data{};
    data.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    data.commandList = commands;
    data.color = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->color.resource));
    data.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->depth.resource));
    data.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->motion.resource));
    data.exposure = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->exposure.resource));
    data.reactive = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->reactive.resource));
    data.transparencyAndComposition = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->transparency.resource));
    data.output = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->output.resource),
        FFX_API_RESOURCE_STATE_UNORDERED_ACCESS);
    data.jitterOffset = {frame->jitter_x, frame->jitter_y};
    data.motionVectorScale = {frame->motion_scale_x, frame->motion_scale_y};
    data.renderSize = {record.render_width, record.render_height};
    data.upscaleSize = {record.output_width, record.output_height};
    data.frameTimeDelta = record.frame_time_ms;
    data.preExposure = frame->pre_exposure;
    data.reset = frame->reset != 0;
    rsf::ffx_sharpening(data, frame->sharpness);
    data.cameraNear = record.camera.near_plane;
    data.cameraFar = far_plane;
    data.cameraFovAngleVertical = record.camera.vertical_fov_radians;
    data.viewSpaceToMetersFactor = frame->view_space_to_meters;
    return static_cast<FsrBindings*>(bindings)->dispatch(context, &data.header) == FFX_API_RETURN_OK ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

struct FsrOps {
    decltype(&load) load_library;
    decltype(&bind) bind_sdk;
    decltype(&unbind) unbind_sdk;
    decltype(&versions) query_versions;
    decltype(&create) create_context;
    decltype(&destroy) destroy_context;
    decltype(&effective_version) query_effective_version;
    decltype(&plan) plan_resolution;
    decltype(&dispatch) evaluate;
};
static_assert(sizeof(FsrOps) == 9 * sizeof(void*));
constexpr FsrOps fsr_ops{load, bind, unbind, versions, create, destroy, effective_version, plan, dispatch};

} // namespace

extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_fsr_open(const rsf_sr_open_desc*, void**, const FsrOps*);

namespace {

rsf_backend_result sr_open(const rsf_sr_open_desc* desc, void** out)
{
    const auto result = rsf::validate_open(desc, out);
    return result == RSF_BACKEND_OK ? rsf_core_d3d12_sr_provider_fsr_open(desc, out, &fsr_ops) : result;
}

#endif

rsf_backend_result fg_create(const rsf_fg_swapchain_desc*, rsf_fg_swapchain_result*, void**) { return absent; }
rsf_backend_result fg_options(void*, uint32_t, uint32_t, rsf_ui_mode) { return absent; }
rsf_backend_result fg_tag(void*, void*, const rsf_fg_frame*) { return absent; }
rsf_backend_result fg_after_present(void*) { return absent; }
rsf_backend_result fg_resize(void*, uint32_t, uint32_t, uint32_t) { return absent; }
rsf_backend_result fg_marker(void*, rsf_latency_marker, uint64_t) { return absent; }
rsf_backend_result fg_sleep(void*) { return absent; }
rsf_backend_result fg_generated(void*, uint64_t*) { return absent; }
void fg_destroy(void*) {}

const rsf_sr_provider sr_provider = {
    sizeof(rsf_sr_provider), probe, sr_open,
#if defined(RSF_HAVE_FFX)
    rsf_core_d3d12_sr_provider_plan, rsf_d3d12_sr_provider_evaluate, rsf_core_d3d12_sr_provider_release,
    rsf_core_d3d12_sr_provider_close, rsf_core_d3d12_sr_provider_version,
#else
    sr_plan, sr_evaluate, sr_release, sr_close, nullptr,
#endif
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,   fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep, fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_fsr_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_fsr_fg_provider(void) { return &fg_provider; }
