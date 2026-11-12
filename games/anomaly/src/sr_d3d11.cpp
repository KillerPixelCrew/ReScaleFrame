// SPDX-License-Identifier: GPL-3.0-only
#include "sr_d3d11.h"
#include "sr_roundtrip_hlsl.h"   // generated from shaders/sr/rsf_sr_roundtrip.hlsl
#include "sr_upscale_hlsl.h"     // generated from shaders/sr/rsf_sr_upscale.hlsl
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <cstring>
#include <iterator>
#include <memory>

namespace rsf::anomaly::sr {
static_assert(sizeof(D3D11_TEXTURE2D_DESC) == texture2d_desc_size);

const char* round_trip_source() noexcept { return rsf_sr_roundtrip_hlsl; }
const char* upscale_source() noexcept { return rsf_sr_upscale_hlsl; }

const char* twin_error_name(TwinError error) noexcept {
    static constexpr const char* names[] = {"none", "not_texture", "shape", "format", "support", "create", "depth"};
    const auto index = static_cast<std::size_t>(error);
    return index < std::size(names) ? names[index] : "unknown";
}

namespace {
bool unorm8(DXGI_FORMAT format) noexcept {
    return format == DXGI_FORMAT_R8G8B8A8_UNORM || format == DXGI_FORMAT_B8G8R8A8_UNORM ||
        format == DXGI_FORMAT_R8G8B8A8_TYPELESS || format == DXGI_FORMAT_B8G8R8A8_TYPELESS;
}
// The common twin builder. width/height 0 keep the engine size. Views follow the engine's own view descriptions when
// given (a typeless engine texture needs them); promoted twins use default views of the FP16 texture.
TwinError create_twin(ID3D11Device* device, ID3D11Resource* engine, ID3D11RenderTargetView* engine_rtv,
    ID3D11ShaderResourceView* engine_srv, ID3D11UnorderedAccessView* engine_uav, bool with_uav, uint32_t width,
    uint32_t height, bool promote, uint32_t d3d_format, Twin& out) noexcept {
    out = {};
    if (!device || !engine) return TwinError::NotTexture;
    ComPtr<ID3D11Texture2D> source;
    if (FAILED(engine->QueryInterface(IID_PPV_ARGS(&source)))) return TwinError::NotTexture;
    D3D11_TEXTURE2D_DESC desc{};
    source->GetDesc(&desc);
    if (!desc.Width || !desc.Height || desc.MipLevels != 1 || desc.ArraySize != 1 || desc.SampleDesc.Count != 1 ||
        desc.Usage != D3D11_USAGE_DEFAULT || (desc.BindFlags & (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE)) !=
            (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE))
        return TwinError::Shape;
    if (promote && !unorm8(desc.Format)) return TwinError::Format;
    const DXGI_FORMAT format = promote ? DXGI_FORMAT_R16G16B16A16_FLOAT : desc.Format;
    UINT support{};
    constexpr UINT required = D3D11_FORMAT_SUPPORT_TEXTURE2D | D3D11_FORMAT_SUPPORT_RENDER_TARGET |
        D3D11_FORMAT_SUPPORT_SHADER_LOAD | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE;
    // Only the promoted twins need blending beyond what the engine format already did; the FP16 format must blend.
    const UINT needed = required | (promote ? UINT(D3D11_FORMAT_SUPPORT_BLENDABLE) : 0u);
    if (promote && (FAILED(device->CheckFormatSupport(format, &support)) || (support & needed) != needed ||
        (with_uav && !(support & D3D11_FORMAT_SUPPORT_TYPED_UNORDERED_ACCESS_VIEW))))
        return TwinError::Support;
    D3D11_TEXTURE2D_DESC twin = desc;
    twin.Width = width ? width : desc.Width;
    twin.Height = height ? height : desc.Height;
    twin.Format = format;
    twin.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE | (with_uav ? D3D11_BIND_UNORDERED_ACCESS : 0u);
    twin.CPUAccessFlags = 0;
    twin.MiscFlags = 0;
    D3D11_RENDER_TARGET_VIEW_DESC rtv_desc{};
    D3D11_SHADER_RESOURCE_VIEW_DESC srv_desc{};
    D3D11_UNORDERED_ACCESS_VIEW_DESC uav_desc{};
    const bool own_views = !promote;
    if (own_views) {
        if (engine_rtv) engine_rtv->GetDesc(&rtv_desc);
        if (engine_srv) engine_srv->GetDesc(&srv_desc);
        if (engine_uav) engine_uav->GetDesc(&uav_desc);
        if ((engine_rtv && rtv_desc.ViewDimension != D3D11_RTV_DIMENSION_TEXTURE2D) ||
            (engine_srv && srv_desc.ViewDimension != D3D11_SRV_DIMENSION_TEXTURE2D) ||
            (engine_uav && uav_desc.ViewDimension != D3D11_UAV_DIMENSION_TEXTURE2D))
            return TwinError::Shape;
    }
    if (FAILED(device->CreateTexture2D(&twin, nullptr, &out.texture)) ||
        FAILED(device->CreateRenderTargetView(out.texture.Get(), own_views && engine_rtv ? &rtv_desc : nullptr, &out.rtv)) ||
        FAILED(device->CreateShaderResourceView(out.texture.Get(), own_views && engine_srv ? &srv_desc : nullptr, &out.srv)) ||
        (with_uav && FAILED(device->CreateUnorderedAccessView(out.texture.Get(), own_views && engine_uav ? &uav_desc : nullptr, &out.uav)))) {
        out = {};
        return TwinError::Create;
    }
    D3D11_TEXTURE2D_DESC created{};
    out.texture->GetDesc(&created);
    auto& f = out.fields;
    f.surface = f.texture_surface = f.desc_cache = out.texture.Get();
    f.rtv = out.rtv.Get();
    f.srv = out.srv.Get();
    f.uav = out.uav.Get();
    f.width = created.Width;
    f.height = created.Height;
    f.format = d3d_format;
    std::memcpy(f.desc.data(), &created, sizeof(created));
    return TwinError::None;
}

// Pipeline objects shared by both passes: no culling, no blending, depth as requested.
bool create_states(ID3D11Device* device, bool write_depth, ComPtr<ID3D11RasterizerState>& raster,
    ComPtr<ID3D11BlendState>& blend, ComPtr<ID3D11DepthStencilState>& depth) noexcept {
    D3D11_RASTERIZER_DESC raster_desc{};
    raster_desc.FillMode = D3D11_FILL_SOLID;
    raster_desc.CullMode = D3D11_CULL_NONE;
    raster_desc.DepthClipEnable = TRUE;
    D3D11_BLEND_DESC blend_desc{};
    blend_desc.IndependentBlendEnable = FALSE;
    blend_desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    D3D11_DEPTH_STENCIL_DESC depth_desc{};
    depth_desc.DepthEnable = write_depth ? TRUE : FALSE;
    depth_desc.DepthWriteMask = write_depth ? D3D11_DEPTH_WRITE_MASK_ALL : D3D11_DEPTH_WRITE_MASK_ZERO;
    depth_desc.DepthFunc = D3D11_COMPARISON_ALWAYS;
    return SUCCEEDED(device->CreateRasterizerState(&raster_desc, &raster)) &&
        SUCCEEDED(device->CreateBlendState(&blend_desc, &blend)) &&
        SUCCEEDED(device->CreateDepthStencilState(&depth_desc, &depth));
}

// Saves the complete pipeline state, binds the common full-screen setup, draws three vertices, restores.
template<class Bind>
bool draw_full_screen(ID3D11DeviceContext* context, ID3D11VertexShader* vertex, ID3D11PixelShader* pixel,
    ID3D11Buffer* constants, ID3D11RasterizerState* raster, ID3D11BlendState* blend, ID3D11DepthStencilState* depth,
    uint32_t width, uint32_t height, Bind&& bind) noexcept {
    // The full scope: the engine may leave geometry/hull/domain shaders, UAVs or linkage bound.
    auto state = std::make_unique<rsf_d3d11_state>();
    if (!rsf_d3d11_state_save(context, state.get())) return false;
    ID3D11ShaderResourceView* null_views[4]{};
    context->PSSetShaderResources(0, 4, null_views);   // no input may stay bound as an output or vice versa
    bind(context);
    context->IASetInputLayout(nullptr);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->VSSetShader(vertex, nullptr, 0);
    context->HSSetShader(nullptr, nullptr, 0);
    context->DSSetShader(nullptr, nullptr, 0);
    context->GSSetShader(nullptr, nullptr, 0);
    context->PSSetShader(pixel, nullptr, 0);
    context->PSSetConstantBuffers(0, 1, &constants);
    context->RSSetState(raster);
    const D3D11_VIEWPORT viewport{0, 0, float(width), float(height), 0, 1};
    context->RSSetViewports(1, &viewport);
    const float factor[4]{};
    context->OMSetBlendState(blend, factor, 0xffffffff);
    context->OMSetDepthStencilState(depth, 0);
    ID3D11Predicate* predicate{};
    BOOL predicate_value{};
    context->GetPredication(&predicate, &predicate_value);
    context->SetPredication(nullptr, FALSE);
    context->Draw(3, 0);
    context->SetPredication(predicate, predicate_value);
    if (predicate) predicate->Release();
    rsf_d3d11_state_restore(context, state.get());
    return true;
}
}

TwinError make_twin(ID3D11Device* device, ID3D11Resource* engine, bool with_uav, Twin& out) noexcept {
    return create_twin(device, engine, nullptr, nullptr, nullptr, with_uav, 0, 0, true, d3dfmt_a16b16g16r16f, out);
}

TwinError make_world_twin(ID3D11Device* device, const TargetFields& engine, uint32_t width, uint32_t height, bool promote,
    Twin& out) noexcept {
    if (!width || !height) { out = {}; return TwinError::Shape; }
    return create_twin(device, static_cast<ID3D11Resource*>(engine.surface), static_cast<ID3D11RenderTargetView*>(engine.rtv),
        static_cast<ID3D11ShaderResourceView*>(engine.srv), static_cast<ID3D11UnorderedAccessView*>(engine.uav),
        engine.uav != nullptr, width, height, promote, promote ? d3dfmt_a16b16g16r16f : engine.format, out);
}

TwinError make_depth_twin(ID3D11Device* device, ID3D11DepthStencilView* engine, uint32_t output_width, uint32_t output_height,
    uint32_t width, uint32_t height, DepthTwin& out) noexcept {
    out = {};
    if (!device || !engine || !width || !height) return TwinError::NotTexture;
    D3D11_DEPTH_STENCIL_VIEW_DESC view{};
    engine->GetDesc(&view);
    if (view.Format != DXGI_FORMAT_D24_UNORM_S8_UINT || view.ViewDimension != D3D11_DSV_DIMENSION_TEXTURE2D || view.Flags)
        return TwinError::Depth;
    ComPtr<ID3D11Resource> resource;
    engine->GetResource(&resource);
    ComPtr<ID3D11Texture2D> texture;
    if (!resource || FAILED(resource.As(&texture))) return TwinError::Depth;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    if (desc.Width != output_width || desc.Height != output_height || desc.SampleDesc.Count != 1 || desc.ArraySize != 1)
        return TwinError::Depth;
    D3D11_TEXTURE2D_DESC twin{width, height, 1, 1, DXGI_FORMAT_R24G8_TYPELESS, {1, 0}, D3D11_USAGE_DEFAULT,
        D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE, 0, 0};
    D3D11_DEPTH_STENCIL_VIEW_DESC dsv{};
    dsv.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
    dsv.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    srv.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    srv.Texture2D.MipLevels = 1;
    if (FAILED(device->CreateTexture2D(&twin, nullptr, &out.texture)) ||
        FAILED(device->CreateDepthStencilView(out.texture.Get(), &dsv, &out.dsv)) ||
        FAILED(device->CreateShaderResourceView(out.texture.Get(), &srv, &out.srv))) {
        out = {};
        return TwinError::Create;
    }
    out.width = width;
    out.height = height;
    return TwinError::None;
}

TwinError make_scratch_target(ID3D11Device* device, ID3D11RenderTargetView* engine, uint32_t width, uint32_t height,
    ScratchTarget& out) noexcept {
    out = {};
    if (!device || !engine || !width || !height) return TwinError::NotTexture;
    D3D11_RENDER_TARGET_VIEW_DESC view{};
    engine->GetDesc(&view);
    if (view.ViewDimension != D3D11_RTV_DIMENSION_TEXTURE2D || view.Format == DXGI_FORMAT_UNKNOWN) return TwinError::Shape;
    D3D11_TEXTURE2D_DESC desc{width, height, 1, 1, view.Format, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET, 0, 0};
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &out.texture)) ||
        FAILED(device->CreateRenderTargetView(out.texture.Get(), nullptr, &out.rtv))) {
        out = {};
        return TwinError::Create;
    }
    return TwinError::None;
}

void set_full_viewport(ID3D11DeviceContext* context, uint32_t width, uint32_t height) noexcept {
    if (!context || !width || !height) return;
    const D3D11_VIEWPORT viewport{0, 0, float(width), float(height), 0, 1};
    context->RSSetViewports(1, &viewport);
}

bool RoundTripPass::create(ID3D11Device* device, void (*log)(void*, const char*), void* user) noexcept try {
    reset();
    if (!device) return false;
    const char* source = round_trip_source();
    if (!rsf::compile_vertex(device, source, "rsf_sr_roundtrip.hlsl", "rsf_roundtrip_vs", &vertex_, log, user) ||
        !rsf::compile_pixel(device, source, "rsf_sr_roundtrip.hlsl", "rsf_roundtrip_ps", &pixel_, log, user)) {
        reset();
        return false;
    }
    D3D11_BUFFER_DESC buffer{16, D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER, 0, 0, 0};
    if (FAILED(device->CreateBuffer(&buffer, nullptr, &constants_)) || !create_states(device, false, raster_, blend_, depth_)) {
        reset();
        return false;
    }
    device_ = device;
    return true;
} catch (...) { reset(); return false; }

void RoundTripPass::reset() noexcept {
    device_.Reset(); vertex_.Reset(); pixel_.Reset(); constants_.Reset();
    raster_.Reset(); blend_.Reset(); depth_.Reset();
}

bool RoundTripPass::run(ID3D11DeviceContext* context, ID3D11ShaderResourceView* source, uint32_t source_width,
    uint32_t source_height, ID3D11RenderTargetView* target, uint32_t width, uint32_t height) noexcept {
    if (!ready() || !context || !source || !target || !width || !height || !source_width || !source_height) return false;
    const uint32_t constants[4]{width, height, source_width, source_height};
    return draw_full_screen(context, vertex_.Get(), pixel_.Get(), constants_.Get(), raster_.Get(), blend_.Get(), depth_.Get(),
        width, height, [&](ID3D11DeviceContext* ctx) {
            ctx->UpdateSubresource(constants_.Get(), 0, nullptr, constants, 0, 0);
            ctx->OMSetRenderTargetsAndUnorderedAccessViews(1, &target, nullptr, 0, 0, nullptr, nullptr);
            ctx->PSSetShaderResources(0, 1, &source);
        });
}

bool UpscalePass::create(ID3D11Device* device, void (*log)(void*, const char*), void* user) noexcept try {
    reset();
    if (!device) return false;
    const char* source = upscale_source();
    if (!rsf::compile_vertex(device, source, "rsf_sr_upscale.hlsl", "rsf_upscale_vs", &vertex_, log, user) ||
        !rsf::compile_pixel(device, source, "rsf_sr_upscale.hlsl", "rsf_upscale_ps", &pixel_, log, user)) {
        reset();
        return false;
    }
    D3D11_BUFFER_DESC buffer{16, D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER, 0, 0, 0};
    if (FAILED(device->CreateBuffer(&buffer, nullptr, &constants_)) || !create_states(device, true, raster_, blend_, depth_)) {
        reset();
        return false;
    }
    // Stages 4 and 5. A failure here leaves the Stage 2 pass usable (temporal_ready() stays false).
    ComPtr<ID3D11RasterizerState> unused_raster;
    ComPtr<ID3D11BlendState> unused_blend;
    if (!rsf::compile_pixel(device, source, "rsf_sr_upscale.hlsl", "rsf_prepare_ps", &prepare_, log, user) ||
        !rsf::compile_pixel(device, source, "rsf_sr_upscale.hlsl", "rsf_reinsert_ps", &reinsert_, log, user) ||
        FAILED(device->CreateBuffer(&buffer, nullptr, &prepare_constants_)) ||
        !create_states(device, false, unused_raster, unused_blend, no_depth_)) {
        prepare_.Reset(); reinsert_.Reset(); prepare_constants_.Reset(); no_depth_.Reset();
    }
    device_ = device;
    return true;
} catch (...) { reset(); return false; }

void UpscalePass::reset() noexcept {
    device_.Reset(); vertex_.Reset(); pixel_.Reset(); constants_.Reset();
    raster_.Reset(); blend_.Reset(); depth_.Reset();
    prepare_.Reset(); reinsert_.Reset(); prepare_constants_.Reset(); no_depth_.Reset();
}

bool UpscalePass::prepare(ID3D11DeviceContext* context, const PrepareViews& v) noexcept {
    if (!temporal_ready() || !context || !v.colour || !v.motion || !v.depth || !v.colour_target || !v.motion_target ||
        !v.width || !v.height)
        return false;
    struct { uint32_t width, height; float depth; uint32_t zero; } constants{v.width, v.height, v.first_person_depth,
        v.first_person_zero ? 1u : 0u};
    static_assert(sizeof(constants) == 16);
    return draw_full_screen(context, vertex_.Get(), prepare_.Get(), constants_.Get(), raster_.Get(), blend_.Get(), no_depth_.Get(),
        v.width, v.height, [&](ID3D11DeviceContext* ctx) {
            ctx->UpdateSubresource(prepare_constants_.Get(), 0, nullptr, &constants, 0, 0);
            ID3D11Buffer* b1 = prepare_constants_.Get();
            ctx->PSSetConstantBuffers(1, 1, &b1);
            ID3D11RenderTargetView* targets[2]{v.colour_target, v.motion_target};
            ctx->OMSetRenderTargetsAndUnorderedAccessViews(2, targets, nullptr, 0, 0, nullptr, nullptr);
            ID3D11ShaderResourceView* null6[6]{};
            ctx->PSSetShaderResources(0, 6, null6);
            ID3D11ShaderResourceView* colour = v.colour;
            ID3D11ShaderResourceView* depth = v.depth;
            ID3D11ShaderResourceView* motion = v.motion;
            ctx->PSSetShaderResources(0, 1, &colour);
            ctx->PSSetShaderResources(3, 1, &depth);
            ctx->PSSetShaderResources(5, 1, &motion);
        });
}

bool UpscalePass::reinsert(ID3D11DeviceContext* context, const UpscaleViews& v, ID3D11ShaderResourceView* reconstructed) noexcept {
    if (!temporal_ready() || !context || !reconstructed || !v.colour || !v.position || !v.distort || !v.depth || !v.colour_target ||
        !v.position_target || !v.distort_target || !v.depth_target || !v.width || !v.height || !v.source_width ||
        !v.source_height || v.source_width > v.width || v.source_height > v.height)
        return false;
    const uint32_t constants[4]{v.width, v.height, v.source_width, v.source_height};
    return draw_full_screen(context, vertex_.Get(), reinsert_.Get(), constants_.Get(), raster_.Get(), blend_.Get(), depth_.Get(),
        v.width, v.height, [&](ID3D11DeviceContext* ctx) {
            ctx->UpdateSubresource(constants_.Get(), 0, nullptr, constants, 0, 0);
            ctx->ClearDepthStencilView(v.depth_target, D3D11_CLEAR_STENCIL, 1.0f, 0);
            ID3D11RenderTargetView* targets[3]{v.colour_target, v.position_target, v.distort_target};
            ctx->OMSetRenderTargetsAndUnorderedAccessViews(3, targets, v.depth_target, 0, 0, nullptr, nullptr);
            ID3D11ShaderResourceView* inputs[5]{v.colour, v.position, v.distort, v.depth, reconstructed};
            ctx->PSSetShaderResources(0, 5, inputs);
        });
}

bool TemporalTargets::matches(ID3D11Device* device, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh, DXGI_FORMAT format) const noexcept {
    if (!colour || !motion || !output || !exposure || !empty_motion) return false;
    ComPtr<ID3D11Device> owner;
    colour->GetDevice(&owner);
    return owner.Get() == device && render_width == rw && render_height == rh && output_width == ow && output_height == oh &&
        output_format == format;
}

TwinError make_temporal_targets(ID3D11Device* device, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh, DXGI_FORMAT output_format,
    TemporalTargets& out) noexcept try {
    out = {};
    if (!device || !rw || !rh || !ow || !oh || rw > ow || rh > oh || output_format == DXGI_FORMAT_UNKNOWN) return TwinError::Shape;
    D3D11_TEXTURE2D_DESC desc{rw, rh, 1, 1, DXGI_FORMAT_R16G16B16A16_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT,
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, 0, 0};
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &out.colour)) || FAILED(device->CreateTexture2D(&desc, nullptr, &out.motion)) ||
        FAILED(device->CreateRenderTargetView(out.colour.Get(), nullptr, &out.colour_rtv)) ||
        FAILED(device->CreateRenderTargetView(out.motion.Get(), nullptr, &out.motion_rtv)) ||
        FAILED(device->CreateShaderResourceView(out.colour.Get(), nullptr, &out.colour_srv)) ||
        FAILED(device->CreateShaderResourceView(out.motion.Get(), nullptr, &out.motion_srv))) {
        out = {};
        return TwinError::Create;
    }
    // Zero-initialised: every pixel invalid, so the pipeline reconstructs camera motion everywhere.
    std::unique_ptr<uint16_t[]> zeros(new uint16_t[std::size_t(rw) * rh * 4]());
    D3D11_TEXTURE2D_DESC empty = desc;
    empty.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    empty.Usage = D3D11_USAGE_IMMUTABLE;
    const D3D11_SUBRESOURCE_DATA zero_data{zeros.get(), rw * 8u, 0};
    D3D11_TEXTURE2D_DESC output{ow, oh, 1, 1, output_format, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_SHADER_RESOURCE, 0, 0};
    const float one = 1.0f;
    const D3D11_SUBRESOURCE_DATA exposure_data{&one, sizeof(one), 0};
    D3D11_TEXTURE2D_DESC exposure{1, 1, 1, 1, DXGI_FORMAT_R32_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_SHADER_RESOURCE, 0, 0};
    if (FAILED(device->CreateTexture2D(&empty, &zero_data, &out.empty_motion)) ||
        FAILED(device->CreateShaderResourceView(out.empty_motion.Get(), nullptr, &out.empty_motion_srv)) ||
        FAILED(device->CreateTexture2D(&output, nullptr, &out.output)) ||
        FAILED(device->CreateShaderResourceView(out.output.Get(), nullptr, &out.output_srv)) ||
        FAILED(device->CreateTexture2D(&exposure, &exposure_data, &out.exposure))) {
        out = {};
        return TwinError::Create;
    }
    out.render_width = rw; out.render_height = rh; out.output_width = ow; out.output_height = oh;
    out.output_format = output_format;
    return TwinError::None;
} catch (...) { out = {}; return TwinError::Create; }

bool UpscalePass::run(ID3D11DeviceContext* context, const UpscaleViews& v) noexcept {
    if (!ready() || !context || !v.colour || !v.position || !v.distort || !v.depth || !v.colour_target ||
        !v.position_target || !v.distort_target || !v.depth_target || !v.width || !v.height || !v.source_width ||
        !v.source_height || v.source_width > v.width || v.source_height > v.height)
        return false;
    const uint32_t constants[4]{v.width, v.height, v.source_width, v.source_height};
    return draw_full_screen(context, vertex_.Get(), pixel_.Get(), constants_.Get(), raster_.Get(), blend_.Get(), depth_.Get(),
        v.width, v.height, [&](ID3D11DeviceContext* ctx) {
            ctx->UpdateSubresource(constants_.Get(), 0, nullptr, constants, 0, 0);
            // The engine depth keeps no world stencil: no pass after the seam tests it (SMAA, the only post pass
            // that enables stencil, binds no depth view), so the stale stencil of the last stock frame is cleared.
            ctx->ClearDepthStencilView(v.depth_target, D3D11_CLEAR_STENCIL, 1.0f, 0);
            ID3D11RenderTargetView* targets[3]{v.colour_target, v.position_target, v.distort_target};
            ctx->OMSetRenderTargetsAndUnorderedAccessViews(3, targets, v.depth_target, 0, 0, nullptr, nullptr);
            ID3D11ShaderResourceView* inputs[4]{v.colour, v.position, v.distort, v.depth};
            ctx->PSSetShaderResources(0, 4, inputs);
        });
}
}
