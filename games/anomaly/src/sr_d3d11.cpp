// SPDX-License-Identifier: GPL-3.0-only
#include "sr_d3d11.h"
#include "sr_roundtrip_hlsl.h"   // generated from shaders/sr/rsf_sr_roundtrip.hlsl
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <cstring>
#include <iterator>
#include <memory>

namespace rsf::anomaly::sr {
static_assert(sizeof(D3D11_TEXTURE2D_DESC) == texture2d_desc_size);

const char* round_trip_source() noexcept { return rsf_sr_roundtrip_hlsl; }

const char* twin_error_name(TwinError error) noexcept {
    static constexpr const char* names[] = {"none", "not_texture", "shape", "format", "support", "create"};
    const auto index = static_cast<std::size_t>(error);
    return index < std::size(names) ? names[index] : "unknown";
}

TwinError make_twin(ID3D11Device* device, ID3D11Resource* engine, bool with_uav, Twin& out) noexcept {
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
    if (desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM && desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM &&
        desc.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS && desc.Format != DXGI_FORMAT_B8G8R8A8_TYPELESS)
        return TwinError::Format;
    UINT support{};
    constexpr UINT required = D3D11_FORMAT_SUPPORT_TEXTURE2D | D3D11_FORMAT_SUPPORT_RENDER_TARGET |
        D3D11_FORMAT_SUPPORT_BLENDABLE | D3D11_FORMAT_SUPPORT_SHADER_LOAD | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE;
    if (FAILED(device->CheckFormatSupport(DXGI_FORMAT_R16G16B16A16_FLOAT, &support)) || (support & required) != required ||
        (with_uav && !(support & D3D11_FORMAT_SUPPORT_TYPED_UNORDERED_ACCESS_VIEW)))
        return TwinError::Support;
    D3D11_TEXTURE2D_DESC twin = desc;
    twin.Format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    twin.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE | (with_uav ? D3D11_BIND_UNORDERED_ACCESS : 0u);
    twin.CPUAccessFlags = 0;
    twin.MiscFlags = 0;
    if (FAILED(device->CreateTexture2D(&twin, nullptr, &out.texture)) ||
        FAILED(device->CreateRenderTargetView(out.texture.Get(), nullptr, &out.rtv)) ||
        FAILED(device->CreateShaderResourceView(out.texture.Get(), nullptr, &out.srv)) ||
        (with_uav && FAILED(device->CreateUnorderedAccessView(out.texture.Get(), nullptr, &out.uav)))) {
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
    f.format = d3dfmt_a16b16g16r16f;
    std::memcpy(f.desc.data(), &created, sizeof(created));
    return TwinError::None;
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
    D3D11_RASTERIZER_DESC raster{};
    raster.FillMode = D3D11_FILL_SOLID;
    raster.CullMode = D3D11_CULL_NONE;
    raster.DepthClipEnable = TRUE;
    D3D11_BLEND_DESC blend{};
    blend.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    D3D11_DEPTH_STENCIL_DESC depth{};
    depth.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    depth.DepthFunc = D3D11_COMPARISON_ALWAYS;
    if (FAILED(device->CreateBuffer(&buffer, nullptr, &constants_)) || FAILED(device->CreateRasterizerState(&raster, &raster_)) ||
        FAILED(device->CreateBlendState(&blend, &blend_)) || FAILED(device->CreateDepthStencilState(&depth, &depth_))) {
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
    // The full scope: the engine may leave geometry/hull/domain shaders, UAVs or linkage bound.
    auto state = std::make_unique<rsf_d3d11_state>();
    if (!rsf_d3d11_state_save(context, state.get())) return false;
    const uint32_t constants[4]{width, height, source_width, source_height};
    context->UpdateSubresource(constants_.Get(), 0, nullptr, constants, 0, 0);
    ID3D11ShaderResourceView* null_view{};
    context->PSSetShaderResources(0, 1, &null_view);
    context->OMSetRenderTargetsAndUnorderedAccessViews(1, &target, nullptr, 0, 0, nullptr, nullptr);
    context->IASetInputLayout(nullptr);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->VSSetShader(vertex_.Get(), nullptr, 0);
    context->HSSetShader(nullptr, nullptr, 0);
    context->DSSetShader(nullptr, nullptr, 0);
    context->GSSetShader(nullptr, nullptr, 0);
    context->PSSetShader(pixel_.Get(), nullptr, 0);
    context->PSSetConstantBuffers(0, 1, constants_.GetAddressOf());
    context->PSSetShaderResources(0, 1, &source);
    context->RSSetState(raster_.Get());
    const D3D11_VIEWPORT viewport{0, 0, float(width), float(height), 0, 1};
    context->RSSetViewports(1, &viewport);
    const float factor[4]{};
    context->OMSetBlendState(blend_.Get(), factor, 0xffffffff);
    context->OMSetDepthStencilState(depth_.Get(), 0);
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
