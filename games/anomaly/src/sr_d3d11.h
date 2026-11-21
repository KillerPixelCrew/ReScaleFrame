// SPDX-License-Identifier: MIT
#pragma once
// D3D11 pieces of the SR stages: twin creation, the Stage 1 f^-1/f round-trip pass and the Stage 2 seam upscale pass.
// No engine memory is read here; the caller passes engine resources it has already validated.
#include "sr_swap.h"
#include "sr_d3d11_platform.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <memory>
#include <utility>

namespace rsf::anomaly::sr {
using Microsoft::WRL::ComPtr;

template<class T, void (*Drop)(T*) noexcept>
struct RustDelete {
    void operator()(T* owner) const noexcept { Drop(owner); }
};
template<class T, void (*Drop)(T*) noexcept>
using RustOwner = std::unique_ptr<T, RustDelete<T, Drop>>;

// The facade exposes typed borrows. The matching Rust owner keeps the COM
// references alive until retirement.
template<class T>
class ResourceView {
public:
    ResourceView() noexcept = default;
    explicit ResourceView(void* pointer) noexcept : pointer_(static_cast<T*>(pointer)) {}
    ResourceView(ResourceView&& other) noexcept : pointer_(std::exchange(other.pointer_, nullptr)) {}
    ResourceView& operator=(ResourceView&& other) noexcept {
        if (this != &other) pointer_ = std::exchange(other.pointer_, nullptr);
        return *this;
    }
    explicit operator bool() const noexcept { return pointer_ != nullptr; }
    T* Get() const noexcept { return pointer_; }
    T* operator->() const noexcept { return pointer_; }
    T* const* GetAddressOf() const noexcept { return &pointer_; }
private:
    T* pointer_{};
};

// The Host drops these views before its diagnostic read of the texture's
// reference count, while retaining the texture until the twin retires.
template<class T, uint32_t Kind>
class TwinView : public ResourceView<T> {
public:
    TwinView() noexcept = default;
    TwinView(rsf_sr_target* owner, void* pointer) noexcept : ResourceView<T>(pointer), owner_(owner) {}
    void Reset() noexcept {
        if (this->Get()) rsf_anomaly_sr_release_twin_view(owner_, Kind);
        ResourceView<T>::operator=(ResourceView<T>{});
    }
private:
    rsf_sr_target* owner_{};
};

enum class TwinError : uint8_t { None, NotTexture, Shape, Format, Support, Create, Depth };
const char* twin_error_name(TwinError) noexcept;

using TargetOwner = RustOwner<rsf_sr_target, rsf_anomaly_sr_drop_target>;
struct Twin {
    ResourceView<ID3D11Texture2D> texture;
    TwinView<ID3D11RenderTargetView, RSF_SR_RTV> rtv;
    TwinView<ID3D11ShaderResourceView, RSF_SR_SRV> srv;
    TwinView<ID3D11UnorderedAccessView, RSF_SR_UAV> uav;
    TargetFields fields{};
private:
    TargetOwner owner_;
    static TwinError create(ID3D11Device*, const rsf_sr_twin_input&, Twin&) noexcept;
    friend TwinError make_twin(ID3D11Device*, ID3D11Resource*, bool, Twin&) noexcept;
    friend TwinError make_world_twin(ID3D11Device*, const TargetFields&, uint32_t, uint32_t, bool, Twin&) noexcept;
};
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
    ResourceView<ID3D11Texture2D> texture;
    ResourceView<ID3D11DepthStencilView> dsv;
    ResourceView<ID3D11ShaderResourceView> srv;
    uint32_t width{}, height{};
private:
    TargetOwner owner_;
    friend TwinError make_depth_twin(ID3D11Device*, ID3D11DepthStencilView*, uint32_t, uint32_t,
        uint32_t, uint32_t, DepthTwin&) noexcept;
};
// `engine` must be a single-sample Texture2D D24_UNORM_S8_UINT view of output_width x output_height (else Depth).
TwinError make_depth_twin(ID3D11Device*, ID3D11DepthStencilView* engine, uint32_t output_width, uint32_t output_height,
    uint32_t width, uint32_t height, DepthTwin& out) noexcept;

// Stage 2 stand-in for HW.pBaseRT inside the world window: a render-size texture with the back buffer view's format.
// phase_occq binds HW.pBaseRT with HW.pBaseZB (colour writes off), which D3D11 rejects at different sizes.
struct ScratchTarget {
    ResourceView<ID3D11Texture2D> texture;
    ResourceView<ID3D11RenderTargetView> rtv;
private:
    TargetOwner owner_;
    friend TwinError make_scratch_target(ID3D11Device*, ID3D11RenderTargetView*, uint32_t, uint32_t, ScratchTarget&) noexcept;
};
TwinError make_scratch_target(ID3D11Device*, ID3D11RenderTargetView* engine, uint32_t width, uint32_t height, ScratchTarget& out) noexcept;

// The full-screen passes save and restore the complete immediate-context pipeline state around their draw, so the
// engine's own binding caches (RCache, StateManager, SRVSManager) stay truthful.
class RoundTripPass {
public:
    bool create(ID3D11Device*, void (*log)(void*, const char*) = nullptr, void* user = nullptr) noexcept;
    bool ready() const noexcept { return bool(owner_); }
    ID3D11Device* device() const noexcept { return static_cast<ID3D11Device*>(rsf_anomaly_sr_pass_device(owner_.get())); }
    // source: FP16 twin SRV (source_width x source_height). target: engine UNORM8 RTV (width x height).
    bool run(ID3D11DeviceContext*, ID3D11ShaderResourceView* source, uint32_t source_width, uint32_t source_height,
        ID3D11RenderTargetView* target, uint32_t width, uint32_t height) noexcept;
    void reset() noexcept;
private:
    RustOwner<rsf_sr_pass, rsf_anomaly_sr_drop_pass> owner_;
};

// Stages 4 and 5 (section 12): the render-size SR inputs the prepare pass writes, the output-size texture the
// pipeline copies its reconstruction into, and the constant 1x1 exposure.
struct TemporalTargets {
    ResourceView<ID3D11Texture2D> colour, motion, output, exposure, empty_motion;
    ResourceView<ID3D11RenderTargetView> colour_rtv, motion_rtv;
    ResourceView<ID3D11ShaderResourceView> colour_srv, motion_srv, output_srv, empty_motion_srv;
private:
    RustOwner<rsf_sr_temporal_targets, rsf_anomaly_sr_drop_temporal> owner_;
    friend TwinError make_temporal_targets(ID3D11Device*, uint32_t, uint32_t, uint32_t, uint32_t, DXGI_FORMAT,
        TemporalTargets&) noexcept;
};
const rsf_sr_d3d11& d3d11_platform() noexcept;
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
    bool ready() const noexcept { return bool(owner_); }
    ID3D11Device* device() const noexcept { return static_cast<ID3D11Device*>(rsf_anomaly_sr_pass_device(owner_.get())); }
    bool run(ID3D11DeviceContext*, const UpscaleViews&) noexcept;
    // Stages 4 and 5. The prepare pass at render size, and the output-size seam pass with the colour taken from the
    // reconstructed linear image (`reconstructed`, output size) instead of the bilinear filter.
    bool temporal_ready() const noexcept { return rsf_anomaly_sr_temporal_ready(owner_.get()) != 0; }
    bool prepare(ID3D11DeviceContext*, const PrepareViews&) noexcept;
    bool reinsert(ID3D11DeviceContext*, const UpscaleViews&, ID3D11ShaderResourceView* reconstructed) noexcept;
    void reset() noexcept;
private:
    RustOwner<rsf_sr_pass, rsf_anomaly_sr_drop_pass> owner_;
};

// What CRender::rmNormal does for a bound target of this size (r4.cpp:852-858).
void set_full_viewport(ID3D11DeviceContext*, uint32_t width, uint32_t height) noexcept;

// The embedded RSF-authored HLSL (games/anomaly/shaders/sr/).
const char* round_trip_source() noexcept;
const char* upscale_source() noexcept;
}
