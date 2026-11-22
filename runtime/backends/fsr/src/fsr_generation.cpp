// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#include "../../common/frame_capture12.h"
#if defined(RSF_HAVE_FFX_FG)
#include <ffx_framegeneration.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
#include <memory>
#include <new>

using Microsoft::WRL::ComPtr;
namespace {
// The SDK records below are generated into d3d12_generation/fsr_abi.rs.
static_assert(sizeof(ffxConfigureDescFrameGeneration) == 144 &&
    sizeof(ffxDispatchDescFrameGenerationPrepareV2) == 232 &&
    sizeof(ffxDispatchDescFrameGeneration) == 312 &&
    sizeof(ffxCallbackDescFrameGenerationPresent) == 192);
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

// The SDK may read these COM-backed swap-chain descriptors until destruction.
struct SwapchainBindings {
    DXGI_SWAP_CHAIN_DESC1 chain_desc{};
    ComPtr<IDXGIFactory2> factory;
    IDXGISwapChain4* chain{};
    ffxCreateContextDescFrameGenerationSwapChainVersionDX12 chain_version{};
    ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 swapchain{};
};

uint32_t surface_format(uint32_t dxgi)
{
    return ffxApiGetSurfaceFormatDX12(static_cast<DXGI_FORMAT>(dxgi));
}

rsf_backend_result create_chain(FARPROC entry, const rsf_generation_setup* setup, void** context, void** chain,
    uint32_t* ffx, void** bindings)
{
    auto storage = std::unique_ptr<SwapchainBindings>(new (std::nothrow) SwapchainBindings{});
    if (!storage) return RSF_BACKEND_ERROR_INIT_FAILED;
    auto& api = *storage;
    const auto created_factory = CreateDXGIFactory2(0, IID_PPV_ARGS(&api.factory));
    if (FAILED(created_factory)) {
        rsf::say(setup->chain.log, setup->chain.log_user, "FSR FG create DXGI factory failed HRESULT=0x%08lx",
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
    const auto create = reinterpret_cast<PfnFfxCreateContext>(reinterpret_cast<void*>(entry));
    *ffx = create(context, &desc.header, nullptr);
    *bindings = storage.release();
    *chain = api.chain;
    return RSF_BACKEND_OK;
}

void close_chain(void* bindings)
{
    delete static_cast<SwapchainBindings*>(bindings);
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
void resource(void* texture, FfxApiResource* out)
{
    *out = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(texture));
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

struct NativeOps {
    const rsf_gen12_checks* check;
    const rsf_core_capture12_platform* capture;
    const rsf_fsr12_sdk* sdk;
    uint32_t generation_version;
    decltype(&create_chain) create_chain_fn;
    decltype(&close_chain) close_chain_fn;
    decltype(&resource) resource_fn;
    decltype(&surface_format) surface_format_fn;
    decltype(&chain_extent) chain_extent_fn;
    decltype(&whole_texture) whole_texture_fn;
    decltype(&copy) copy_fn;
    decltype(&present_count) present_count_fn;
};
static_assert(sizeof(NativeOps) == 12 * sizeof(void*));

const NativeOps& native_ops()
{
    static const rsf_fsr12_sdk sdk{
        {uint32_t(FFX_FRAMEGENERATION_VERSION_MAJOR), uint32_t(FFX_FRAMEGENERATION_VERSION_MINOR),
            uint32_t(FFX_FRAMEGENERATION_VERSION_PATCH)},
        {uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MAJOR),
            uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MINOR),
            uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_PATCH)}};
    static const NativeOps ops{&rsf::generation12_checks, rsf::FrameCapture12::native_platform(), &sdk,
        FFX_FRAMEGENERATION_VERSION, create_chain, close_chain, resource, surface_format,
        chain_extent, whole_texture, copy, present_count};
    return ops;
}
}

extern "C" rsf_backend_result rsf_core_fsr12_create_native(const NativeOps*, const rsf_generation_setup*, void**, void**);
extern "C" uint32_t rsf_core_fsr12_probe_native(const rsf_gen12_checks*, const rsf_generation_setup*);

extern "C" const rsf_generation_provider* rsf_generation_fsr()
{
    static const auto provider = [] {
        auto value = *rsf_core_fsr12_provider();
        value.create = [](const rsf_generation_setup* setup, void** out, void** chain) {
            return rsf_core_fsr12_create_native(&native_ops(), setup, out, chain);
        };
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
    return rsf_core_fsr12_probe_native(&rsf::generation12_checks, &setup);
}
#else
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return rsf::not_compiled_provider(); }
extern "C" uint32_t rsf_generation_fsr4_supported(void*, const char*) { return 0; }
#endif
