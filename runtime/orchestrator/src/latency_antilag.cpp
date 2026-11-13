// SPDX-License-Identifier: GPL-3.0-only
#include "latency_antilag.h"
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi.h>
#ifdef RSF_HAVE_ANTILAG2
#include <ffx_antilag2_dx11.h>
#include <ffx_antilag2_dx12.h>
#endif
namespace rsf::latency {
namespace {
class Driver final : public AntiLagDriver {
#ifdef RSF_HAVE_ANTILAG2
    AMD::AntiLag2DX11::Context dx11_{};
    AMD::AntiLag2DX12::Context dx12_{};
    uint32_t api_{};
#endif
public:
    bool initialize(uint32_t api, void* device) noexcept override {
#ifdef RSF_HAVE_ANTILAG2
        api_ = api;
        return (api == 11 ? AMD::AntiLag2DX11::Initialize(&dx11_) :
            AMD::AntiLag2DX12::Initialize(&dx12_, static_cast<ID3D12Device*>(device))) == S_OK;
#else
        (void)api; (void)device; return false;
#endif
    }
    bool update(bool enabled, uint32_t max_fps) noexcept override {
#ifdef RSF_HAVE_ANTILAG2
        return SUCCEEDED(api_ == 11 ? AMD::AntiLag2DX11::Update(&dx11_, enabled, max_fps) :
            AMD::AntiLag2DX12::Update(&dx12_, enabled, max_fps));
#else
        (void)enabled; (void)max_fps; return false;
#endif
    }
    bool end_render(void* chain, bool enabled, bool generated) noexcept override {
#ifdef RSF_HAVE_ANTILAG2
        // FSR 3.1.1+ reads this SDK-defined private data and marks every real/generated presentation itself.
        static constexpr GUID key{0x5083ae5b,0x8070,0x4fca,{0x8e,0xe5,0x35,0x82,0xdd,0x36,0x7d,0x13}};
        struct Data { AMD::AntiLag2DX12::Context* context; bool enabled; } data{&dx12_, enabled && generated};
        const auto attached = static_cast<IDXGISwapChain*>(chain)->SetPrivateData(key, sizeof(data), &data);
        return SUCCEEDED(attached) && (!enabled || !generated || SUCCEEDED(AMD::AntiLag2DX12::MarkEndOfFrameRendering(&dx12_)));
#else
        (void)chain; (void)enabled; (void)generated; return false;
#endif
    }
    void shutdown() noexcept override {
#ifdef RSF_HAVE_ANTILAG2
        if (dx11_.m_pAntiLagAPI) AMD::AntiLag2DX11::DeInitialize(&dx11_);
        if (dx12_.m_pAntiLagAPI) AMD::AntiLag2DX12::DeInitialize(&dx12_);
        dx11_={}; dx12_={}; api_=0;
#endif
    }
};
}
std::unique_ptr<AntiLagDriver> make_antilag_driver() { return std::make_unique<Driver>(); }
}
