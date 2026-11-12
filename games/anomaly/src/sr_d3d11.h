// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// D3D11 pieces of SR Stage 1: FP16 twin creation and the f^-1/f round-trip pass. No engine memory is read here;
// the caller passes engine resources it has already validated.
#include "sr_swap.h"
#include <d3d11.h>
#include <wrl/client.h>

namespace rsf::anomaly::sr {
using Microsoft::WRL::ComPtr;

// An RSF-owned FP16 twin of one engine colour target, with the field values the swap installs.
struct Twin {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11RenderTargetView> rtv;
    ComPtr<ID3D11ShaderResourceView> srv;
    ComPtr<ID3D11UnorderedAccessView> uav;
    TargetFields fields;
};
enum class TwinError : uint8_t { None, NotTexture, Shape, Format, Support, Create };
const char* twin_error_name(TwinError) noexcept;
// Same size as `engine`, R16G16B16A16_FLOAT, render target + shader resource (+ UAV when `with_uav`).
// The engine texture must be a single-sample, single-mip, single-slice UNORM8 render target.
TwinError make_twin(ID3D11Device*, ID3D11Resource* engine, bool with_uav, Twin& out) noexcept;

// The reinsertion pass. Saves and restores the complete immediate-context pipeline state around its draw, so the
// engine's own binding caches (RCache, StateManager, SRVSManager) stay truthful.
class RoundTripPass {
public:
    bool create(ID3D11Device*, void (*log)(void*, const char*) = nullptr, void* user = nullptr) noexcept;
    bool ready() const noexcept { return pixel_ != nullptr; }
    ID3D11Device* device() const noexcept { return device_.Get(); }
    // source: FP16 twin SRV (source_width x source_height). target: engine UNORM8 RTV (width x height).
    bool run(ID3D11DeviceContext*, ID3D11ShaderResourceView* source, uint32_t source_width, uint32_t source_height,
        ID3D11RenderTargetView* target, uint32_t width, uint32_t height) noexcept;
    void reset() noexcept;
private:
    ComPtr<ID3D11Device> device_;
    ComPtr<ID3D11VertexShader> vertex_;
    ComPtr<ID3D11PixelShader> pixel_;
    ComPtr<ID3D11Buffer> constants_;
    ComPtr<ID3D11RasterizerState> raster_;
    ComPtr<ID3D11BlendState> blend_;
    ComPtr<ID3D11DepthStencilState> depth_;
};
// The embedded RSF-authored HLSL (games/anomaly/shaders/sr/rsf_sr_roundtrip.hlsl).
const char* round_trip_source() noexcept;
}
