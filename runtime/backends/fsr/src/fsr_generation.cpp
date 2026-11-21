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
    uint32_t feature_major{};
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

rsf_backend_result resolve(const rsf_generation_setup* setup, void* handle, void*, void** out)
{
    auto api = std::unique_ptr<Bindings>(new (std::nothrow) Bindings{});
    if (!api) return RSF_BACKEND_ERROR_INIT_FAILED;
    const auto module = static_cast<HMODULE>(handle);
    api->create = rsf::entry<PfnFfxCreateContext>(module, "ffxCreateContext");
    api->destroy = rsf::entry<PfnFfxDestroyContext>(module, "ffxDestroyContext");
    api->configure = rsf::entry<PfnFfxConfigure>(module, "ffxConfigure");
    api->dispatch = rsf::entry<PfnFfxDispatch>(module, "ffxDispatch");
    api->query = rsf::entry<PfnFfxQuery>(module, "ffxQuery");
    if (!api->create || !api->destroy || !api->configure || !api->dispatch || !api->query)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    api->log = setup->chain.log;
    api->log_user = setup->chain.log_user;
    api->feature_major = setup->feature_major;
    api->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    api->backend.device = static_cast<ID3D12Device*>(setup->chain.d3d12_device);
    api->backend.header.pNext = &api->override_version.header;
    api->override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
    api->generation_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_VERSION;
    api->generation_version.version = FFX_FRAMEGENERATION_VERSION;
    api->generation_version.header.pNext = &api->backend.header;
    api->generation.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    api->generation.header.pNext = &api->generation_version.header;
    api->generation.flags = (setup->depth_inverted ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED : 0u) |
        (setup->depth_infinite ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE : 0u) |
        (setup->motion_jittered ? FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u) |
        (setup->motion_at_display_resolution ? FFX_FRAMEGENERATION_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS : 0u) |
        (setup->hdr ? FFX_FRAMEGENERATION_ENABLE_HIGH_DYNAMIC_RANGE : 0u);
    *out = api.release();
    return RSF_BACKEND_OK;
}

void close(void* bindings)
{
    delete static_cast<Bindings*>(bindings);
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
    if (!query || query(nullptr, &versions.header) != FFX_API_RETURN_OK || *count > rsf::max_ffx_versions)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    for (uint64_t i = 0; i < *count; ++i)
        out[i] = rsf::ffx_version(ids[i], names[i]);
    return RSF_BACKEND_OK;
}

rsf_backend_result versions(void* bindings, const rsf_generation_setup* setup, rsf_ffx_version* out, uint64_t* count)
{
    return query_versions(static_cast<Bindings*>(bindings)->query, *setup, out, count);
}

rsf_backend_result probe_versions(void* module, const rsf_generation_setup* setup, rsf_ffx_version* out, uint64_t* count)
{
    const auto query = rsf::entry<PfnFfxQuery>(static_cast<HMODULE>(module), "ffxQuery");
    return query_versions(query, *setup, out, count);
}

uint32_t surface_format(uint32_t dxgi)
{
    return ffxApiGetSurfaceFormatDX12(static_cast<DXGI_FORMAT>(dxgi));
}

rsf_backend_result create_generation(void* bindings, const rsf_gen12_extent* extent,
    const rsf_fg_status* status, const char* directory, void** out)
{
    auto& api = *static_cast<Bindings*>(bindings);
    api.override_version.versionId = status->version_id;
    auto& desc = api.generation;
    desc.displaySize = {extent->width, extent->height};
    desc.maxRenderSize = desc.displaySize;
    desc.backBufferFormat = extent->format;
    const auto result = api.create(out, &desc.header, nullptr);
    if (directory) rsf::say(api.log, api.log_user,
        "FSR FG create generation: FFX result=%u requested_major=%u selected=%s id=%llu API=%u.%u.%u root=%s",
        result, api.feature_major, status->version_name, static_cast<unsigned long long>(status->version_id),
        uint32_t(FFX_FRAMEGENERATION_VERSION_MAJOR), uint32_t(FFX_FRAMEGENERATION_VERSION_MINOR),
        uint32_t(FFX_FRAMEGENERATION_VERSION_PATCH), directory);
    return result == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}

rsf_backend_result provider_matches(void* bindings, void* context, uint64_t version)
{
    ffxQueryGetProviderVersion actual{};
    actual.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    return static_cast<Bindings*>(bindings)->query(&context, &actual.header) == FFX_API_RETURN_OK &&
        actual.versionId == version ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_SUPPORTED;
}

rsf_backend_result create_chain(void* bindings, const rsf_generation_setup* setup, void** context, void** chain)
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
    const auto result = api.create(context, &desc.header, nullptr);
    rsf::say(api.log, api.log_user, "FSR FG create swapchain: FFX result=%u chain=%u API=%u.%u.%u size=%ux%u buffers=%u format=%u",
        result, uint32_t(api.chain != nullptr), uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MAJOR),
        uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MINOR), uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_PATCH),
        api.chain_desc.Width, api.chain_desc.Height, api.chain_desc.BufferCount, uint32_t(api.chain_desc.Format));
    *chain = result == FFX_API_RETURN_OK ? api.chain : nullptr;
    return result == FFX_API_RETURN_OK && *chain ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
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
bool native_state(uint32_t state, D3D12_RESOURCE_STATES& out)
{
    out = D3D12_RESOURCE_STATE_COMMON;
    const uint32_t known = FFX_API_RESOURCE_STATE_COMMON | FFX_API_RESOURCE_STATE_PRESENT |
        FFX_API_RESOURCE_STATE_UNORDERED_ACCESS | FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ |
        FFX_API_RESOURCE_STATE_COPY_SRC | FFX_API_RESOURCE_STATE_COPY_DEST |
        FFX_API_RESOURCE_STATE_INDIRECT_ARGUMENT | FFX_API_RESOURCE_STATE_RENDER_TARGET |
        FFX_API_RESOURCE_STATE_DEPTH_ATTACHMENT;
    if (state & ~known) return false;
    if (state & FFX_API_RESOURCE_STATE_UNORDERED_ACCESS) out |= D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    if (state & FFX_API_RESOURCE_STATE_COMPUTE_READ) out |= D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    if (state & FFX_API_RESOURCE_STATE_PIXEL_READ) out |= D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    if (state & FFX_API_RESOURCE_STATE_COPY_SRC) out |= D3D12_RESOURCE_STATE_COPY_SOURCE;
    if (state & FFX_API_RESOURCE_STATE_COPY_DEST) out |= D3D12_RESOURCE_STATE_COPY_DEST;
    if (state & FFX_API_RESOURCE_STATE_INDIRECT_ARGUMENT) out |= D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT;
    if (state & FFX_API_RESOURCE_STATE_RENDER_TARGET) out |= D3D12_RESOURCE_STATE_RENDER_TARGET;
    if (state & FFX_API_RESOURCE_STATE_DEPTH_ATTACHMENT) out |= D3D12_RESOURCE_STATE_DEPTH_WRITE;
    return true;
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
    if (config->user) {
        desc.frameGenerationCallback = generate_callback;
        desc.frameGenerationCallbackUserContext = config->user;
    }
    if (config->copy_present) {
        desc.presentCallback = present_callback;
        desc.presentCallbackUserContext = config->user;
    }
    desc.generationRect = {0, 0, static_cast<int32_t>(config->width), static_cast<int32_t>(config->height)};
    if (config->hudless) desc.HUDLessColor = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(config->hudless));
    return static_cast<Bindings*>(bindings)->configure(&context, &desc.header);
}

uint32_t prepare(void* bindings, void* context, void* command, const rsf_fg_frame* frame,
    uint64_t sequence, uint32_t reset, float meters, float forward_sign)
{
    const auto& record = *frame->record;
    ffxDispatchDescFrameGenerationPrepareV2 data{};
    data.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2;
    data.frameID = sequence;
    data.commandList = command;
    data.renderSize = {record.render_width, record.render_height};
    data.jitterOffset = {record.camera.jitter_pixels[0], record.camera.jitter_pixels[1]};
    data.motionVectorScale = {frame->motion_scale_x, frame->motion_scale_y};
    data.frameTimeDelta = record.frame_time_ms;
    data.cameraNear = record.camera.near_plane;
    data.cameraFar = record.camera.far_plane;
    data.cameraFovAngleVertical = record.camera.vertical_fov_radians;
    data.viewSpaceToMetersFactor = meters;
    data.reset = reset != 0;
    data.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->depth.resource));
    data.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->motion.resource));
    for (uint32_t i = 0; i < 3; ++i) {
        data.cameraPosition[i] = record.camera.view_to_world[12 + i];
        data.cameraRight[i] = record.camera.view_to_world[i];
        data.cameraUp[i] = record.camera.view_to_world[4 + i];
        data.cameraForward[i] = forward_sign * record.camera.view_to_world[8 + i];
    }
    return static_cast<Bindings*>(bindings)->dispatch(&context, &data.header);
}

rsf_fsr12_capture_resource capture_resource(const FfxApiResource& resource)
{
    D3D12_RESOURCE_STATES state{};
    if (!native_state(resource.state, state)) return {};
    return {resource.resource, uint32_t(state)};
}

uint32_t generate(void* bindings, void* context, void* raw, rsf_fsr12_generated* out)
{
    auto* desc = static_cast<ffxDispatchDescFrameGeneration*>(raw);
    const auto result = static_cast<Bindings*>(bindings)->dispatch(&context, &desc->header);
    out->command = desc->commandList;
    out->sequence = desc->frameID;
    out->count = desc->numGeneratedFrames;
    out->reset = desc->reset;
    if (result == FFX_API_RETURN_OK) {
        out->input = capture_resource(desc->presentColor);
        for (uint32_t i = 0; i < desc->numGeneratedFrames && i < 4; ++i)
            out->outputs[i] = capture_resource(desc->outputs[i]);
    }
    return result;
}

uint32_t copy_present(void* raw)
{
    const auto* desc = static_cast<const ffxCallbackDescFrameGenerationPresent*>(raw);
    auto* list = static_cast<ID3D12GraphicsCommandList*>(desc->commandList);
    auto* source = static_cast<ID3D12Resource*>(desc->currentBackBuffer.resource);
    auto* output = static_cast<ID3D12Resource*>(desc->outputSwapChainBuffer.resource);
    D3D12_RESOURCE_STATES source_state{}, output_state{};
    if (!source || !output || !list || !native_state(desc->currentBackBuffer.state, source_state) ||
        !native_state(desc->outputSwapChainBuffer.state, output_state)) return FFX_API_RETURN_ERROR_PARAMETER;
    if (source != output) rsf::copy_transitioned(list, output, output_state, output_state, source, source_state, source_state);
    return FFX_API_RETURN_OK;
}

int32_t present_count(void* chain, uint32_t* count)
{
    return static_cast<IDXGISwapChain4*>(chain)->GetLastPresentCount(count);
}

rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    static const rsf_fsr12_ops ops{&rsf::generation12_checks, rsf::FrameCapture12::native_platform(),
        resolve, close, versions, surface_format, create_generation, provider_matches, create_chain,
        destroy_context, wait, chain_extent, whole_texture, configure, prepare, generate, copy_present, present_count};
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
