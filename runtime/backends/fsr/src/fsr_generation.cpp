// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#include "../../common/frame_capture12.h"
#include "ffx_version.h"
#if defined(RSF_HAVE_FFX_FG)
#include <ffx_framegeneration.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
#include <memory>
#include <new>

using Microsoft::WRL::ComPtr;
namespace {
static_assert(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40);
static_assert(FFX_API_RETURN_ERROR_PARAMETER == 6);
// Rust plans the create flags (d3d12_generation/fsr.rs CREATE_*).
static_assert(FFX_FRAMEGENERATION_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS == 2 &&
    FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION == 4 &&
    FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED == 8 && FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE == 16 &&
    FFX_FRAMEGENERATION_ENABLE_HIGH_DYNAMIC_RANGE == 32);
// Rust maps raw FFX resource states to D3D12 states (d3d12_generation/fsr.rs STATE_MAP).
static_assert(FFX_API_RESOURCE_STATE_COMMON == 1 && FFX_API_RESOURCE_STATE_UNORDERED_ACCESS == 2 &&
    FFX_API_RESOURCE_STATE_COMPUTE_READ == 4 && FFX_API_RESOURCE_STATE_PIXEL_READ == 8 &&
    FFX_API_RESOURCE_STATE_COPY_SRC == 16 && FFX_API_RESOURCE_STATE_COPY_DEST == 32 &&
    FFX_API_RESOURCE_STATE_INDIRECT_ARGUMENT == 64 && FFX_API_RESOURCE_STATE_PRESENT == 128 &&
    FFX_API_RESOURCE_STATE_RENDER_TARGET == 256 && FFX_API_RESOURCE_STATE_DEPTH_ATTACHMENT == 512);
static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_RENDER_TARGET == 0x4 &&
    D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 0x8 && D3D12_RESOURCE_STATE_DEPTH_WRITE == 0x10 &&
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE == 0x80 && D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT == 0x200 &&
    D3D12_RESOURCE_STATE_COPY_DEST == 0x400 && D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800);

// The SDK requires pointers passed in create descriptors to remain live
// until context destruction. Rust owns both contexts and this backing storage.
struct Bindings {
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxConfigure configure{};
    PfnFfxDispatch dispatch{};
    PfnFfxQuery query{};
    rsf_backend_log_fn log{};
    void* log_user{};
    ffxCreateContextDescFrameGeneration generation{};
    ffxCreateContextDescFrameGenerationVersion generation_version{};
    ffxCreateBackendDX12Desc backend{};
    ffxOverrideVersion override_version{};
    DXGI_SWAP_CHAIN_DESC1 chain_desc{};
    ComPtr<IDXGIFactory2> factory;
    IDXGISwapChain4* chain{};
    ffxCreateContextDescFrameGenerationSwapChainVersionDX12 chain_version{};
    ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 swapchain{};
};

// Rust resolves and admits the SDK exports, then binds them with `bind`.
rsf_backend_result resolve(const rsf_generation_setup* setup, void*, void*, void** out)
{
    auto api = std::unique_ptr<Bindings>(new (std::nothrow) Bindings{});
    if (!api) return RSF_BACKEND_ERROR_INIT_FAILED;
    api->log = setup->chain.log;
    api->log_user = setup->chain.log_user;
    api->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    api->backend.device = static_cast<ID3D12Device*>(setup->chain.d3d12_device);
    api->backend.header.pNext = &api->override_version.header;
    api->override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
    api->generation_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_VERSION;
    api->generation_version.version = FFX_FRAMEGENERATION_VERSION;
    api->generation_version.header.pNext = &api->backend.header;
    api->generation.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    api->generation.header.pNext = &api->generation_version.header;
    *out = api.release();
    return RSF_BACKEND_OK;
}

void close(void* bindings)
{
    delete static_cast<Bindings*>(bindings);
}

void bind(void* bindings, const rsf_fsr12_entry* entries)
{
    auto& api = *static_cast<Bindings*>(bindings);
    api.create = reinterpret_cast<PfnFfxCreateContext>(entries[RSF_FSR12_CREATE_CONTEXT]);
    api.destroy = reinterpret_cast<PfnFfxDestroyContext>(entries[RSF_FSR12_DESTROY_CONTEXT]);
    api.query = reinterpret_cast<PfnFfxQuery>(entries[RSF_FSR12_QUERY]);
    api.dispatch = reinterpret_cast<PfnFfxDispatch>(entries[RSF_FSR12_DISPATCH]);
    api.configure = reinterpret_cast<PfnFfxConfigure>(entries[RSF_FSR12_CONFIGURE]);
}

rsf_backend_result query_versions(PfnFfxQuery query, const rsf_generation_setup& setup,
    rsf_ffx_version* out, uint64_t* count)
{
    uint64_t ids[rsf::max_ffx_versions]{};
    const char* names[rsf::max_ffx_versions]{};
    *count = rsf::max_ffx_versions;
    ffxQueryDescGetVersions versions{};
    versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    versions.device = setup.chain.d3d12_device;
    versions.outputCount = count;
    versions.versionIds = ids;
    versions.versionNames = names;
    if (query(nullptr, &versions.header) != FFX_API_RETURN_OK || *count > rsf::max_ffx_versions)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    for (uint64_t i = 0; i < *count; ++i)
        out[i] = rsf::ffx_version(ids[i], names[i]);
    return RSF_BACKEND_OK;
}

rsf_backend_result versions(void* bindings, const rsf_generation_setup* setup, rsf_ffx_version* out, uint64_t* count)
{
    return query_versions(static_cast<Bindings*>(bindings)->query, *setup, out, count);
}

// Rust passes the ffxQuery export it resolved from the probed module.
rsf_backend_result probe_versions(void* query, const rsf_generation_setup* setup, rsf_ffx_version* out, uint64_t* count)
{
    return query_versions(reinterpret_cast<PfnFfxQuery>(query), *setup, out, count);
}

uint32_t surface_format(uint32_t dxgi)
{
    return ffxApiGetSurfaceFormatDX12(static_cast<DXGI_FORMAT>(dxgi));
}

uint32_t create_generation(void* bindings, const rsf_fsr12_generation* plan, void** out)
{
    auto& api = *static_cast<Bindings*>(bindings);
    api.override_version.versionId = plan->version_id;
    auto& desc = api.generation;
    desc.flags = plan->flags;
    desc.displaySize = {plan->width, plan->height};
    desc.maxRenderSize = {plan->max_render_width, plan->max_render_height};
    desc.backBufferFormat = plan->format;
    return api.create(out, &desc.header, nullptr);
}

uint32_t provider_version(void* bindings, void* context, uint64_t* version)
{
    ffxQueryGetProviderVersion actual{};
    actual.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    const auto result = static_cast<Bindings*>(bindings)->query(&context, &actual.header);
    *version = actual.versionId;
    return result;
}

rsf_backend_result create_chain(void* bindings, const rsf_generation_setup* setup, void** context, void** chain,
    uint32_t* ffx)
{
    auto& api = *static_cast<Bindings*>(bindings);
    const auto created_factory = CreateDXGIFactory2(0, IID_PPV_ARGS(&api.factory));
    if (FAILED(created_factory)) {
        rsf::say(api.log, api.log_user, "FSR FG create DXGI factory failed HRESULT=0x%08lx",
            static_cast<unsigned long>(created_factory));
        return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    api.chain_desc = rsf::chain_description(setup->chain);
    api.chain_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_VERSION_DX12;
    api.chain_version.version = FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION;
    auto& desc = api.swapchain;
    desc.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12;
    desc.header.pNext = &api.chain_version.header;
    desc.swapchain = &api.chain;
    desc.hwnd = static_cast<HWND>(setup->chain.hwnd);
    desc.desc = &api.chain_desc;
    desc.dxgiFactory = api.factory.Get();
    desc.gameQueue = static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue);
    *ffx = api.create(context, &desc.header, nullptr);
    *chain = api.chain;
    return RSF_BACKEND_OK;
}

uint32_t destroy_context(void* bindings, void** context)
{
    return static_cast<Bindings*>(bindings)->destroy(context, nullptr);
}

uint32_t wait(void* bindings, void* context)
{
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsDX12 desc{};
    desc.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_WAIT_FOR_PRESENTS_DX12;
    return static_cast<Bindings*>(bindings)->dispatch(&context, &desc.header);
}

int32_t chain_extent(void* chain, rsf_gen12_extent* out)
{
    DXGI_SWAP_CHAIN_DESC1 desc{};
    const auto result = static_cast<IDXGISwapChain4*>(chain)->GetDesc1(&desc);
    if (SUCCEEDED(result)) *out = {desc.Width, desc.Height, ffxApiGetSurfaceFormatDX12(desc.Format)};
    return result;
}

uint32_t whole_texture(const rsf_backend_resource* resource)
{
    const auto desc = static_cast<ID3D12Resource*>(resource->resource)->GetDesc();
    return !resource->x && !resource->y && resource->width == desc.Width && resource->height == desc.Height;
}
ffxReturnCode_t generate_callback(ffxDispatchDescFrameGeneration* desc, void* user)
{
    return rsf_core_fsr12_generate(user, desc);
}

ffxReturnCode_t present_callback(ffxCallbackDescFrameGenerationPresent* desc, void* user)
{
    return rsf_core_fsr12_present(user, desc, uint32_t(desc->isGeneratedFrame));
}

uint32_t configure(void* bindings, void* context, const rsf_fsr12_config* config)
{
    ffxConfigureDescFrameGeneration desc{};
    desc.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    desc.swapChain = config->chain;
    desc.frameID = config->sequence;
    desc.frameGenerationEnabled = config->enabled != 0;
    if (config->generation_callback) {
        desc.frameGenerationCallback = generate_callback;
        desc.frameGenerationCallbackUserContext = config->user;
    }
    if (config->present_callback) {
        desc.presentCallback = present_callback;
        desc.presentCallbackUserContext = config->user;
    }
    desc.generationRect = {config->left, config->top, config->width, config->height};
    if (config->hudless) desc.HUDLessColor = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(config->hudless));
    return static_cast<Bindings*>(bindings)->configure(&context, &desc.header);
}

uint32_t prepare(void* bindings, void* context, const rsf_fsr12_prepare* plan)
{
    ffxDispatchDescFrameGenerationPrepareV2 data{};
    data.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2;
    data.frameID = plan->sequence;
    data.commandList = plan->command;
    data.renderSize = {plan->render_width, plan->render_height};
    data.jitterOffset = {plan->jitter[0], plan->jitter[1]};
    data.motionVectorScale = {plan->motion_scale[0], plan->motion_scale[1]};
    data.frameTimeDelta = plan->frame_time_ms;
    data.cameraNear = plan->near_plane;
    data.cameraFar = plan->far_plane;
    data.cameraFovAngleVertical = plan->vertical_fov;
    data.viewSpaceToMetersFactor = plan->view_space_to_meters;
    data.reset = plan->reset != 0;
    data.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(plan->depth));
    data.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(plan->motion));
    for (uint32_t i = 0; i < 3; ++i) {
        data.cameraPosition[i] = plan->position[i];
        data.cameraRight[i] = plan->right[i];
        data.cameraUp[i] = plan->up[i];
        data.cameraForward[i] = plan->forward[i];
    }
    return static_cast<Bindings*>(bindings)->dispatch(&context, &data.header);
}

rsf_fsr12_capture_resource raw_resource(const FfxApiResource& resource)
{
    return {resource.resource, resource.state};
}

uint32_t generate(void* bindings, void* context, void* raw, rsf_fsr12_generated* out)
{
    auto* desc = static_cast<ffxDispatchDescFrameGeneration*>(raw);
    static_assert(sizeof(ffxDispatchDescFrameGeneration::outputs) == 4 * sizeof(FfxApiResource));
    const auto result = static_cast<Bindings*>(bindings)->dispatch(&context, &desc->header);
    out->command = desc->commandList;
    out->sequence = desc->frameID;
    out->count = desc->numGeneratedFrames;
    out->reset = desc->reset;
    out->input = raw_resource(desc->presentColor);
    for (uint32_t i = 0; i < 4; ++i) out->outputs[i] = raw_resource(desc->outputs[i]);
    return result;
}

void present_facts(void* raw, rsf_fsr12_present* out)
{
    const auto* desc = static_cast<const ffxCallbackDescFrameGenerationPresent*>(raw);
    *out = {desc->commandList, raw_resource(desc->currentBackBuffer), raw_resource(desc->outputSwapChainBuffer)};
}

// Each resource returns to the state it arrived in.
void copy(void* command, void* output, uint32_t output_state, void* source, uint32_t source_state)
{
    const auto output_native = static_cast<D3D12_RESOURCE_STATES>(output_state);
    const auto source_native = static_cast<D3D12_RESOURCE_STATES>(source_state);
    rsf::copy_transitioned(static_cast<ID3D12GraphicsCommandList*>(command), static_cast<ID3D12Resource*>(output),
        output_native, output_native, static_cast<ID3D12Resource*>(source), source_native, source_native);
}

int32_t present_count(void* chain, uint32_t* count)
{
    return static_cast<IDXGISwapChain4*>(chain)->GetLastPresentCount(count);
}

rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    static const rsf_fsr12_sdk sdk{
        {uint32_t(FFX_FRAMEGENERATION_VERSION_MAJOR), uint32_t(FFX_FRAMEGENERATION_VERSION_MINOR),
            uint32_t(FFX_FRAMEGENERATION_VERSION_PATCH)},
        {uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MAJOR),
            uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MINOR),
            uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_PATCH)}};
    static const rsf_fsr12_ops ops{&rsf::generation12_checks, rsf::FrameCapture12::native_platform(), &sdk,
        resolve, close, bind, versions, surface_format, create_generation, provider_version, create_chain,
        destroy_context, wait, chain_extent, whole_texture, configure, prepare, generate, present_facts, copy,
        present_count};
    return rsf_core_fsr12_create(&ops, setup, out, chain);
}
}

extern "C" const rsf_generation_provider* rsf_generation_fsr()
{
    static const auto provider = [] {
        auto value = *rsf_core_fsr12_provider();
        value.create = create;
        return value;
    }();
    return &provider;
}

extern "C" uint32_t rsf_generation_fsr4_supported(void* device, const char* directory)
{
    if (!device || !directory || !*directory) return 0;
    rsf_generation_setup setup{};
    setup.runtime_directory_utf8 = directory;
    setup.chain.d3d12_device = device;
    setup.feature_major = 4;
    return rsf_core_fsr12_probe(&rsf::generation12_checks, &setup, probe_versions);
}
#else
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return rsf::not_compiled_provider(); }
extern "C" uint32_t rsf_generation_fsr4_supported(void*, const char*) { return 0; }
#endif
