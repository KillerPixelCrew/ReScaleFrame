// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// D3D11 pieces of the SR stages: twin creation, the Stage 1 f^-1/f round-trip pass and the Stage 2 seam upscale pass.
// No engine memory is read here; the caller passes engine resources it has already validated.
#include "sr_swap.h"
#include <d3d11.h>
#include <wrl/client.h>

namespace rsf::anomaly::sr {
using Microsoft::WRL::ComPtr;

// An RSF-owned twin of one engine colour target, with the field values the swap installs.
struct Twin {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11RenderTargetView> rtv;
    ComPtr<ID3D11ShaderResourceView> srv;
    ComPtr<ID3D11UnorderedAccessView> uav;
    TargetFields fields;
};
enum class TwinError : uint8_t { None, NotTexture, Shape, Format, Support, Create, Depth };
const char* twin_error_name(TwinError) noexcept;
// Stage 1: same size as `engine`, R16G16B16A16_FLOAT, render target + shader resource (+ UAV when `with_uav`).
// The engine texture must be a single-sample, single-mip, single-slice UNORM8 render target.
TwinError make_twin(ID3D11Device*, ID3D11Resource* engine, bool with_uav, Twin& out) noexcept;
// Stage 2: a `width` x `height` twin of the engine target described by `engine` (surface, rtv, srv, uav, format).
// promote: R16G16B16A16_FLOAT (engine must be UNORM8) with default views. Otherwise the engine format, with views
// created from the engine views' own descriptions, and `fields.format` = the engine D3DFORMAT.
TwinError make_world_twin(ID3D11Device*, const TargetFields& engine, uint32_t width, uint32_t height, bool promote, Twin& out) noexcept;

// Stage 2 depth twin for HW.pBaseZB: R24G8_TYPELESS with a D24_UNORM_S8_UINT depth view and an R24_UNORM_X8_TYPELESS
// shader view (the engine's own CRT::create choice for D24S8, dx10SH_RT.cpp:79-82).
struct DepthTwin {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11DepthStencilView> dsv;
    ComPtr<ID3D11ShaderResourceView> srv;
    uint32_t width{}, height{};
};
// `engine` must be a single-sample Texture2D D24_UNORM_S8_UINT view of output_width x output_height (else Depth).
TwinError make_depth_twin(ID3D11Device*, ID3D11DepthStencilView* engine, uint32_t output_width, uint32_t output_height,
    uint32_t width, uint32_t height, DepthTwin& out) noexcept;

// Stage 2 stand-in for HW.pBaseRT inside the world window: a render-size texture with the back buffer view's format.
// phase_occq binds HW.pBaseRT with HW.pBaseZB (colour writes off), which D3D11 rejects at different sizes.
struct ScratchTarget {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11RenderTargetView> rtv;
};
TwinError make_scratch_target(ID3D11Device*, ID3D11RenderTargetView* engine, uint32_t width, uint32_t height, ScratchTarget& out) noexcept;

// The full-screen passes save and restore the complete immediate-context pipeline state around their draw, so the
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

// Stages 4 and 5 (section 12): the render-size SR inputs the prepare pass writes, the output-size texture the
// pipeline copies its reconstruction into, and the constant 1x1 exposure.
struct TemporalTargets {
    ComPtr<ID3D11Texture2D> colour, motion, output, exposure, empty_motion;
    ComPtr<ID3D11RenderTargetView> colour_rtv, motion_rtv;
    ComPtr<ID3D11ShaderResourceView> colour_srv, motion_srv, output_srv, empty_motion_srv;
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
    DXGI_FORMAT output_format{};
    bool matches(ID3D11Device* device, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh, DXGI_FORMAT format) const noexcept;
};
// colour and motion: render size, R16G16B16A16_FLOAT, RTV + SRV. output: output size in `output_format` (the
// pipeline's output texture format), SRV. exposure: 1x1 R32_FLOAT = 1.0. empty_motion: render size, all zero (every
// pixel invalid), bound when no motion target exists this frame.
TwinError make_temporal_targets(ID3D11Device*, uint32_t render_width, uint32_t render_height, uint32_t output_width,
    uint32_t output_height, DXGI_FORMAT output_format, TemporalTargets& out) noexcept;

struct PrepareViews {
    ID3D11ShaderResourceView* colour{};      // FP16 generic0 twin (engine low tonemap space)
    ID3D11ShaderResourceView* motion{};      // motion target, or TemporalTargets::empty_motion_srv
    ID3D11ShaderResourceView* depth{};       // depth twin, R24_UNORM_X8
    uint32_t width{}, height{};              // render size
    ID3D11RenderTargetView* colour_target{}; // TemporalTargets::colour_rtv
    ID3D11RenderTargetView* motion_target{}; // TemporalTargets::motion_rtv
    float first_person_depth{0.02f};
    bool first_person_zero{true};
};

// Stage 2 seam pass (shaders/sr/rsf_sr_upscale.hlsl): render-size twins in, engine output-size targets out.
struct UpscaleViews {
    ID3D11ShaderResourceView* colour{};      // FP16 generic0 twin
    ID3D11ShaderResourceView* position{};    // rt_Position twin
    ID3D11ShaderResourceView* distort{};     // generic1 twin
    ID3D11ShaderResourceView* depth{};       // depth twin, R24_UNORM_X8
    uint32_t source_width{}, source_height{};
    ID3D11RenderTargetView* colour_target{};     // engine generic0 (UNORM8)
    ID3D11RenderTargetView* position_target{};   // engine rt_Position (FP16)
    ID3D11RenderTargetView* distort_target{};    // engine generic1 (UNORM8)
    ID3D11DepthStencilView* depth_target{};      // engine HW.pBaseZB
    uint32_t width{}, height{};
};
class UpscalePass {
public:
    bool create(ID3D11Device*, void (*log)(void*, const char*) = nullptr, void* user = nullptr) noexcept;
    bool ready() const noexcept { return pixel_ != nullptr; }
    ID3D11Device* device() const noexcept { return device_.Get(); }
    bool run(ID3D11DeviceContext*, const UpscaleViews&) noexcept;
    // Stages 4 and 5. The prepare pass at render size, and the output-size seam pass with the colour taken from the
    // reconstructed linear image (`reconstructed`, output size) instead of the bilinear filter.
    bool temporal_ready() const noexcept { return prepare_ && reinsert_; }
    bool prepare(ID3D11DeviceContext*, const PrepareViews&) noexcept;
    bool reinsert(ID3D11DeviceContext*, const UpscaleViews&, ID3D11ShaderResourceView* reconstructed) noexcept;
    void reset() noexcept;
private:
    ComPtr<ID3D11Device> device_;
    ComPtr<ID3D11VertexShader> vertex_;
    ComPtr<ID3D11PixelShader> pixel_, prepare_, reinsert_;
    ComPtr<ID3D11Buffer> constants_, prepare_constants_;
    ComPtr<ID3D11DepthStencilState> no_depth_;
    ComPtr<ID3D11RasterizerState> raster_;
    ComPtr<ID3D11BlendState> blend_;
    ComPtr<ID3D11DepthStencilState> depth_;
};

// What CRender::rmNormal does for a bound target of this size (r4.cpp:852-858).
void set_full_viewport(ID3D11DeviceContext*, uint32_t width, uint32_t height) noexcept;

// The embedded RSF-authored HLSL (games/anomaly/shaders/sr/).
const char* round_trip_source() noexcept;
const char* upscale_source() noexcept;
}
