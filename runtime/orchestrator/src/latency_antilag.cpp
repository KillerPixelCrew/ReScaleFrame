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
struct SdkContexts {
    AMD::AntiLag2DX11::Context dx11{};
    AMD::AntiLag2DX12::Context dx12{};
};

constexpr rsf::latency::AntiLagNativeOps sdk{
    []() noexcept -> void* { return new (std::nothrow) SdkContexts{}; },
    [](void* raw) noexcept -> int32_t {
        return AMD::AntiLag2DX11::Initialize(&static_cast<SdkContexts*>(raw)->dx11);
    },
    [](void* raw, void* device) noexcept -> int32_t {
        return AMD::AntiLag2DX12::Initialize(&static_cast<SdkContexts*>(raw)->dx12,
            static_cast<ID3D12Device*>(device));
    },
    [](void* raw, uint32_t enabled, uint32_t fps) noexcept -> int32_t {
        return AMD::AntiLag2DX11::Update(&static_cast<SdkContexts*>(raw)->dx11, enabled != 0, fps);
    },
    [](void* raw, uint32_t enabled, uint32_t fps) noexcept -> int32_t {
        return AMD::AntiLag2DX12::Update(&static_cast<SdkContexts*>(raw)->dx12, enabled != 0, fps);
    },
    [](void* raw) noexcept -> void* { return &static_cast<SdkContexts*>(raw)->dx12; },
    [](void* raw) noexcept -> int32_t {
        return AMD::AntiLag2DX12::MarkEndOfFrameRendering(&static_cast<SdkContexts*>(raw)->dx12);
    },
    [](void* raw) noexcept {
        auto& ctx = *static_cast<SdkContexts*>(raw);
        if (ctx.dx11.m_pAntiLagAPI) AMD::AntiLag2DX11::DeInitialize(&ctx.dx11);
        if (ctx.dx12.m_pAntiLagAPI) AMD::AntiLag2DX12::DeInitialize(&ctx.dx12);
        ctx = {};
    },
};
}
#endif

extern "C" void* rsf_native_antilag_context_create() noexcept {
#ifdef RSF_HAVE_ANTILAG2
    return rsf::latency::rsf_core_antilag_create_sdk(&sdk);
#else
    return rsf::latency::rsf_core_antilag_create_sdk(nullptr);
#endif
}
