// SPDX-License-Identifier: MIT
#include "latency_antilag.h"
#ifdef RSF_HAVE_ANTILAG2
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi.h>
#include <ffx_antilag2_dx11.h>
#include <ffx_antilag2_dx12.h>
#include <new>

namespace {
using rsf::latency::AntiLagCallbacks;
struct SdkContexts {
    AMD::AntiLag2DX11::Context dx11{};
    AMD::AntiLag2DX12::Context dx12{};
};

uint32_t initialize(void* raw, uint32_t api, void* device) noexcept {
    auto& sdk = *static_cast<SdkContexts*>(raw);
    return (api == 11 ? AMD::AntiLag2DX11::Initialize(&sdk.dx11) :
        AMD::AntiLag2DX12::Initialize(&sdk.dx12, static_cast<ID3D12Device*>(device))) == S_OK;
}

uint32_t update(void* raw, uint32_t api, uint32_t enabled, uint32_t max_fps) noexcept {
    auto& sdk = *static_cast<SdkContexts*>(raw);
    return SUCCEEDED(api == 11 ? AMD::AntiLag2DX11::Update(&sdk.dx11, enabled != 0, max_fps) :
        AMD::AntiLag2DX12::Update(&sdk.dx12, enabled != 0, max_fps));
}

uint32_t end_render(void* raw, void* chain, uint32_t attach) noexcept {
    auto& sdk = *static_cast<SdkContexts*>(raw);
    // FSR 3.1.1+ reads this SDK-defined private data to type real and generated presentations.
    static constexpr GUID key{0x5083ae5b,0x8070,0x4fca,{0x8e,0xe5,0x35,0x82,0xdd,0x36,0x7d,0x13}};
    struct Data { AMD::AntiLag2DX12::Context* context; bool enabled; } data{&sdk.dx12, attach != 0};
    const auto attached = static_cast<IDXGISwapChain*>(chain)->SetPrivateData(key, sizeof(data), &data);
    return SUCCEEDED(attached) && (attach == 0 || SUCCEEDED(AMD::AntiLag2DX12::MarkEndOfFrameRendering(&sdk.dx12)));
}

void shutdown(void* raw) noexcept {
    auto& sdk = *static_cast<SdkContexts*>(raw);
    if (sdk.dx11.m_pAntiLagAPI) AMD::AntiLag2DX11::DeInitialize(&sdk.dx11);
    if (sdk.dx12.m_pAntiLagAPI) AMD::AntiLag2DX12::DeInitialize(&sdk.dx12);
    sdk = {};
}

constexpr AntiLagCallbacks calls{initialize, update, end_render, shutdown};
}
#endif

extern "C" void* rsf_native_antilag_context_create() noexcept {
#ifdef RSF_HAVE_ANTILAG2
    // DXGI private data embeds the DX12 context pointer. Keep its address
    // for the life of the process. Rust retires the SDK handles separately.
    auto* sdk = new (std::nothrow) SdkContexts{};
    if (!sdk) return nullptr;
    auto* context = rsf::latency::rsf_core_antilag_create(&calls, sdk);
    if (!context) delete sdk;
    return context;
#else
    return rsf::latency::rsf_core_antilag_create(nullptr, nullptr);
#endif
}
