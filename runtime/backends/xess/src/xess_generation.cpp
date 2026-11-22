// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#if defined(RSF_HAVE_XESS_FG)
#include "../../common/d3d12_helpers.h"
#include <xess_fg/xefg_swapchain_d3d12.h>
#include <xell/xell_d3d12.h>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;
namespace {
// The SDK records below are generated into d3d12_generation/xess_abi.rs.
static_assert(sizeof(xell_sleep_params_t) == 8 && sizeof(xefg_swapchain_version_t) == 8 &&
    sizeof(xefg_swapchain_frame_constant_data_t) == 152 && sizeof(xefg_swapchain_properties_t) == 40 &&
    sizeof(xefg_swapchain_present_status_t) == 12 && sizeof(xefg_swapchain_d3d12_resource_data_t) == 40);
static_assert(RSF_LATENCY_INPUT_SAMPLE == 6);
static_assert(RSF_REFLEX_ON == 1);
static_assert(XEFG_SWAPCHAIN_RESULT_SUCCESS == 0 && XELL_RESULT_SUCCESS == 0);

rsf_backend_result create_chain(FARPROC init_entry, FARPROC chain_entry, void* raw, const rsf_generation_setup* setup,
    uint32_t reserved, void** out)
{
    const auto init_chain = reinterpret_cast<decltype(&xefgSwapChainD3D12InitFromSwapChainDesc)>(reinterpret_cast<void*>(init_entry));
    const auto chain_ptr = reinterpret_cast<decltype(&xefgSwapChainD3D12GetSwapChainPtr)>(reinterpret_cast<void*>(chain_entry));
    const auto context = static_cast<xefg_swapchain_handle_t>(raw);
    ComPtr<IDXGIFactory2> factory;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return RSF_BACKEND_ERROR_INIT_FAILED;
    auto desc = rsf::chain_description(setup->chain);
    xefg_swapchain_d3d12_init_params_t init{};
    init.maxInterpolatedFrames = reserved;
    init.initFlags = (setup->depth_inverted ? XEFG_SWAPCHAIN_INIT_FLAG_INVERTED_DEPTH : 0u) |
        (setup->motion_jittered ? XEFG_SWAPCHAIN_INIT_FLAG_JITTERED_MV : 0u);
    init.uiMode = setup->chain.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS_UITEXTURE :
        setup->chain.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS : XEFG_SWAPCHAIN_UI_MODE_NONE;
    if (init_chain(context, static_cast<HWND>(setup->chain.hwnd), &desc, nullptr,
        static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue), factory.Get(), &init) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    ComPtr<IDXGISwapChain4> chain;
    if (chain_ptr(context, IID_PPV_ARGS(&chain)) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = chain.Detach();
    return RSF_BACKEND_OK;
}

void* create_fence(void* device)
{
    ComPtr<ID3D12Fence> fence;
    if (FAILED(static_cast<ID3D12Device*>(device)->CreateFence(0, D3D12_FENCE_FLAG_NONE,
        IID_PPV_ARGS(&fence)))) return nullptr;
    return fence.Detach();
}

void* create_event()
{
    return CreateEventW(nullptr, FALSE, FALSE, nullptr);
}

int32_t drain(void* queue, void* fence, void* event, uint64_t serial)
{
    auto* native_fence = static_cast<ID3D12Fence*>(fence);
    const auto signal = static_cast<ID3D12CommandQueue*>(queue)->Signal(native_fence, serial);
    if (FAILED(signal)) return signal;
    const auto waited = rsf::wait_fence(native_fence, serial, static_cast<HANDLE>(event));
    if (FAILED(waited)) return waited;
    // A previous timed-out registration can still signal the reused event.
    const auto completed = native_fence->GetCompletedValue();
    if (completed == UINT64_MAX) return DXGI_ERROR_DEVICE_REMOVED;
    return completed >= serial ? S_OK : DXGI_ERROR_DEVICE_HUNG;
}

struct NativeOps {
    const rsf_gen12_checks* check;
    decltype(&create_chain) create_chain_fn;
    decltype(&create_fence) create_fence_fn;
    decltype(&create_event) create_event_fn;
    decltype(&drain) drain_fn;
};
static_assert(sizeof(NativeOps) == 5 * sizeof(void*));
const NativeOps native_ops{&rsf::generation12_checks, create_chain, create_fence, create_event, drain};
}

extern "C" rsf_backend_result rsf_core_xeg12_create_native(const NativeOps*, const rsf_generation_setup*, void**, void**);

extern "C" const rsf_generation_provider* rsf_generation_xess()
{
    static const auto provider = [] {
        auto value = *rsf_core_xeg12_provider();
        value.create = [](const rsf_generation_setup* setup, void** out, void** chain) {
            return rsf_core_xeg12_create_native(&native_ops, setup, out, chain);
        };
        return value;
    }();
    return &provider;
}
#else
extern "C" const rsf_generation_provider* rsf_generation_xess() { return rsf::not_compiled_provider(); }
#endif
