/* SPDX-License-Identifier: MIT */
/* XeSS-SR through the official DX12 API. Native D3D11 and frame generation are unavailable
   in this implementation. Build capability is separate from runtime and adapter support. */

#include <rescaleframe/backend.h>
#include <rescaleframe/xess_backend.h>
#include "../../common/sr_helpers.h"
#if defined(RSF_HAVE_XESS)
#include "../../common/sr12_platform.h"
#include <xess/xess_d3d12.h>
#include <memory>
#include <new>
#endif

namespace {

rsf_backend_result probe(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps)
{
    const auto begun = rsf::probe_begin(desc, caps, RSF_VENDOR_INTEL, "XeSS");
    if (begun != RSF_BACKEND_OK) return begun;

#if !defined(RSF_HAVE_XESS)
    caps->available = 0;
    caps->refusal_utf8 = "built without the XeSS headers";
    rsf::probe_say(desc, "xess: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    rsf::probe_say(desc, "xess: SR compiled in, D3D12; native D3D11 and generation unavailable");
    caps->supported_lifetimes = 1u << RSF_LIFETIME_ONLY_NOW;
    caps->available = 1;
    return RSF_BACKEND_OK;
#endif
}

#if !defined(RSF_HAVE_XESS)
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_COMPILED;
#else
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_READY;
#endif

#if defined(RSF_HAVE_XESS)

struct XessBindings {
    decltype(&xessD3D12CreateContext) create{};
    decltype(&xessD3D12Init) init{};
    decltype(&xessD3D12Execute) execute{};
    decltype(&xessDestroyContext) destroy{};
    decltype(&xessGetInputResolution) resolution{};
    decltype(&xessSetVelocityScale) velocity{};
    decltype(&xessGetVersion) get_version{};
};

// RSF quality order, for the typed SDK calls in initialize and plan.
constexpr xess_quality_settings_t qualities[] = {
    XESS_QUALITY_SETTING_AA, XESS_QUALITY_SETTING_QUALITY, XESS_QUALITY_SETTING_BALANCED,
    XESS_QUALITY_SETTING_PERFORMANCE, XESS_QUALITY_SETTING_ULTRA_PERFORMANCE, XESS_QUALITY_SETTING_ULTRA_QUALITY,
};
static_assert(RSF_QUALITY_NATIVE == 0 && RSF_QUALITY_QUALITY == 1 && RSF_QUALITY_BALANCED == 2 &&
    RSF_QUALITY_PERFORMANCE == 3 && RSF_QUALITY_ULTRA_PERFORMANCE == 4 && RSF_QUALITY_ULTRA_QUALITY == 5);

void* load(const rsf_sr_open_desc* desc)
{
    return rsf::load_runtime(*desc, L"libxess.dll");
}

rsf_backend_result bind(void* module, void** out)
{
    auto sdk = std::unique_ptr<XessBindings>(new (std::nothrow) XessBindings{});
    if (!sdk) return RSF_BACKEND_ERROR_INIT_FAILED;
    const auto library = static_cast<HMODULE>(module);
#define RSF_XESS_ENTRY(member, function) sdk->member = rsf::entry<decltype(&function)>(library, #function)
    RSF_XESS_ENTRY(create, xessD3D12CreateContext);
    RSF_XESS_ENTRY(init, xessD3D12Init);
    RSF_XESS_ENTRY(execute, xessD3D12Execute);
    RSF_XESS_ENTRY(destroy, xessDestroyContext);
    RSF_XESS_ENTRY(resolution, xessGetInputResolution);
    RSF_XESS_ENTRY(velocity, xessSetVelocityScale);
    RSF_XESS_ENTRY(get_version, xessGetVersion);
#undef RSF_XESS_ENTRY
    if (!sdk->create || !sdk->init || !sdk->execute || !sdk->destroy || !sdk->resolution ||
        !sdk->velocity || !sdk->get_version)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    *out = sdk.release();
    return RSF_BACKEND_OK;
}

void unbind(void* bindings)
{
    delete static_cast<XessBindings*>(bindings);
}

rsf_backend_result version(void* bindings, uint16_t* out)
{
    xess_version_t reported{};
    if (static_cast<XessBindings*>(bindings)->get_version(&reported) != XESS_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    out[0] = reported.major;
    out[1] = reported.minor;
    out[2] = reported.patch;
    out[3] = reported.reserved;
    return RSF_BACKEND_OK;
}

rsf_backend_result create(void* bindings, void* device, void** out)
{
    xess_context_handle_t context{};
    const auto result = static_cast<XessBindings*>(bindings)->create(static_cast<ID3D12Device*>(device), &context);
    *out = context;
    return result == XESS_RESULT_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_SUPPORTED;
}

void destroy(void* bindings, void* context)
{
    static_cast<XessBindings*>(bindings)->destroy(static_cast<xess_context_handle_t>(context));
}

rsf_backend_result create_mask(void* device, const uint32_t* extent, void** out)
{
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC zero{};
    zero.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    zero.Width = extent[0];
    zero.Height = extent[1];
    zero.DepthOrArraySize = zero.MipLevels = 1;
    zero.SampleDesc.Count = 1;
    zero.Format = DXGI_FORMAT_R8_UNORM;
    ID3D12Resource* resource = nullptr;
    // Committed resources are zero-filled unless created with CREATE_NOT_ZEROED.
    const auto result = static_cast<ID3D12Device*>(device)->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE,
        &zero, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, nullptr, IID_PPV_ARGS(&resource));
    *out = resource;
    return SUCCEEDED(result) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}

void release_mask(void* resource)
{
    static_cast<ID3D12Resource*>(resource)->Release();
}

rsf_backend_result initialize(void* bindings, void* context, const rsf_sr_open_desc* desc)
{
    xess_d3d12_init_params_t init{};
    init.outputResolution = {desc->output_width, desc->output_height};
    init.qualitySetting = qualities[desc->quality];
    init.initFlags = (desc->inverted_depth ? XESS_INIT_FLAG_INVERTED_DEPTH : 0u) |
        (!desc->hdr ? XESS_INIT_FLAG_LDR_INPUT_COLOR : 0u) |
        (desc->motion_jittered ? XESS_INIT_FLAG_JITTERED_MV : 0u) |
        (desc->auto_exposure ? XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE : XESS_INIT_FLAG_EXPOSURE_SCALE_TEXTURE) |
        XESS_INIT_FLAG_RESPONSIVE_PIXEL_MASK;
    return static_cast<XessBindings*>(bindings)->init(static_cast<xess_context_handle_t>(context), &init) ==
        XESS_RESULT_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}

rsf_backend_result plan(void* bindings, void* context, const uint32_t* extent, rsf_quality quality, uint32_t* out)
{
    const xess_2d_t output{extent[0], extent[1]};
    xess_2d_t input{};
    if (static_cast<XessBindings*>(bindings)->resolution(static_cast<xess_context_handle_t>(context),
            &output, qualities[quality], &input) != XESS_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    out[0] = input.x;
    out[1] = input.y;
    return RSF_BACKEND_OK;
}

rsf_backend_result velocity(void* bindings, void* context, float x, float y)
{
    return static_cast<XessBindings*>(bindings)->velocity(static_cast<xess_context_handle_t>(context), x, y) ==
        XESS_RESULT_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

rsf_backend_result execute(void* bindings, void* context, void* commands, const rsf_sr_frame* frame,
    void* mask, float exposure_scale)
{
    xess_d3d12_execute_params_t params{};
    params.pColorTexture = static_cast<ID3D12Resource*>(frame->color.resource);
    params.pDepthTexture = static_cast<ID3D12Resource*>(frame->depth.resource);
    params.pVelocityTexture = static_cast<ID3D12Resource*>(frame->motion.resource);
    params.pExposureScaleTexture = static_cast<ID3D12Resource*>(frame->exposure.resource);
    params.pResponsivePixelMaskTexture = static_cast<ID3D12Resource*>(mask);
    params.pOutputTexture = static_cast<ID3D12Resource*>(frame->output.resource);
    params.inputWidth = frame->record->render_width;
    params.inputHeight = frame->record->render_height;
    params.jitterOffsetX = frame->jitter_x;
    params.jitterOffsetY = frame->jitter_y;
    params.exposureScale = exposure_scale;
    params.resetHistory = frame->reset;
    return static_cast<XessBindings*>(bindings)->execute(static_cast<xess_context_handle_t>(context),
        static_cast<ID3D12GraphicsCommandList*>(commands), &params) == XESS_RESULT_SUCCESS ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

struct XessOps {
    decltype(&load) load_library;
    decltype(&bind) bind_sdk;
    decltype(&unbind) unbind_sdk;
    decltype(&version) get_version;
    decltype(&create) create_context;
    decltype(&destroy) destroy_context;
    decltype(&create_mask) allocate_mask;
    decltype(&release_mask) free_mask;
    decltype(&initialize) initialize_context;
    decltype(&plan) plan_resolution;
    decltype(&velocity) set_velocity;
    decltype(&execute) evaluate;
};
static_assert(sizeof(XessOps) == 12 * sizeof(void*));
constexpr XessOps xess_ops{load, bind, unbind, version, create, destroy, create_mask, release_mask,
    initialize, plan, velocity, execute};

} // namespace

extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_xess_open(const rsf_sr_open_desc*, void**, const XessOps*);

namespace {

rsf_backend_result sr_open(const rsf_sr_open_desc* desc, void** out)
{
    const auto result = rsf::validate_open(desc, out);
    return result == RSF_BACKEND_OK ? rsf_core_d3d12_sr_provider_xess_open(desc, out, &xess_ops) : result;
}

#else
rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return absent; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*) { return absent; }
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*) { return absent; }
rsf_backend_result sr_release(void*) { return absent; }
void sr_close(void*) {}
#endif

/* Scaffolding: the legacy rsf_fg_provider vtable. Frame generation is reached through
   rsf_generation_xess() in xess_generation.cpp, so these only answer for callers of the old
   contract. */
rsf_backend_result fg_create(const rsf_fg_swapchain_desc*, rsf_fg_swapchain_result*, void**)
{
    return absent;
}
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
#if defined(RSF_HAVE_XESS)
    rsf_core_d3d12_sr_provider_plan, rsf_d3d12_sr_provider_evaluate, rsf_core_d3d12_sr_provider_release,
    rsf_core_d3d12_sr_provider_close, rsf_core_d3d12_sr_provider_version,
#else
    sr_plan, sr_evaluate, sr_release, sr_close, nullptr,
#endif
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,     fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep,  fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_xess_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_xess_fg_provider(void) { return &fg_provider; }
