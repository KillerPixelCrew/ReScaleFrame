// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/depth_scale.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <new>
#include <memory>

namespace {
using Microsoft::WRL::ComPtr;
constexpr const char* shader = R"(
Texture2D<float> Depth : register(t0);
Texture2D<uint2> Stencil : register(t1);
cbuffer Params : register(b0) {
    float4 SourceRect;
    float4 OutputRect;
    float2 Jitter;
    uint Bit;
    uint Padding;
};
float4 vertex_main(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
int2 source_position(float4 position) {
    float2 uv = (position.xy - OutputRect.xy) / (OutputRect.zw - OutputRect.xy);
    float2 p = floor(SourceRect.xy + uv * (SourceRect.zw - SourceRect.xy) - Jitter);
    return int2(clamp(p, SourceRect.xy, SourceRect.zw - 1));
}
float depth_main(float4 position : SV_Position) : SV_Depth {
    return Depth.Load(int3(source_position(position), 0));
}
void stencil_main(float4 position : SV_Position) {
    if ((Stencil.Load(int3(source_position(position), 0)).y & Bit) == 0) discard;
}
)";
struct Constants {
    float source[4], output[4], jitter[2];
    uint32_t bit, padding;
};
static_assert(sizeof(Constants) == 48);
struct Bindings {
    ID3D11DeviceContext* context;
    rsf_d3d11_state saved{};
    bool valid;
    explicit Bindings(ID3D11DeviceContext* c) : context(c), valid(rsf_d3d11_state_save(c, &saved) != 0) {}
    ~Bindings() { if (valid) rsf_d3d11_state_restore(context, &saved); }
};
bool rect_valid(const int32_t* r, const D3D11_TEXTURE2D_DESC& d) {
    return r[0] >= 0 && r[1] >= 0 && r[2] > r[0] && r[3] > r[1] &&
        uint32_t(r[2]) <= d.Width && uint32_t(r[3]) <= d.Height;
}
}

struct rsf_depth_scale {
    DWORD thread{};
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11VertexShader> vertex;
    ComPtr<ID3D11PixelShader> depth_shader, stencil_shader;
    ComPtr<ID3D11DepthStencilState> depth_state;
    std::array<ComPtr<ID3D11DepthStencilState>, 8> stencil_states;
    ComPtr<ID3D11RasterizerState> raster;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11Texture2D> original, snapshot, output;
    ComPtr<ID3D11ShaderResourceView> depth_view, stencil_view;
    ComPtr<ID3D11DepthStencilView> output_view;
    UINT width{}, height{};
    bool committed{};
};

extern "C" int32_t rsf_depth_scale_create(void* raw_context, const rsf_depth_scale_setup* setup,
    rsf_depth_scale** out) try
{
    if (out) *out = nullptr;
    if (!raw_context || !setup || !out || setup->struct_size < sizeof(*setup)) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_DEPTH_SCALE_ABI_VERSION) return RSF_DEPTH_SCALE_ABI_MISMATCH;
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    auto p = std::make_unique<rsf_depth_scale>();
    p->thread = GetCurrentThreadId(); p->context = context; context->GetDevice(&p->device);
    auto* device = p->device.Get();
    if (device->GetFeatureLevel() < D3D_FEATURE_LEVEL_11_0 ||
        !rsf::compile_vertex(device, shader, "depth_scale", "vertex_main", &p->vertex, setup->log, setup->log_user) ||
        !rsf::compile_pixel(device, shader, "depth_scale", "depth_main", &p->depth_shader, setup->log, setup->log_user) ||
        !rsf::compile_pixel(device, shader, "depth_scale", "stencil_main", &p->stencil_shader, setup->log, setup->log_user))
        return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    D3D11_DEPTH_STENCIL_DESC ds{};
    ds.DepthEnable = TRUE; ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; ds.DepthFunc = D3D11_COMPARISON_ALWAYS;
    ds.StencilEnable = TRUE; ds.StencilReadMask = ds.StencilWriteMask = 0xff;
    ds.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_REPLACE, D3D11_COMPARISON_ALWAYS};
    ds.BackFace = ds.FrontFace;
    if (FAILED(device->CreateDepthStencilState(&ds, &p->depth_state))) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    ds.DepthEnable = FALSE; ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    for (unsigned i = 0; i < 8; ++i) {
        ds.StencilWriteMask = BYTE(1u << i);
        if (FAILED(device->CreateDepthStencilState(&ds, &p->stencil_states[i]))) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    }
    D3D11_RASTERIZER_DESC rs{}; rs.FillMode = D3D11_FILL_SOLID; rs.CullMode = D3D11_CULL_NONE; rs.DepthClipEnable = TRUE;
    D3D11_BUFFER_DESC cb{}; cb.ByteWidth = sizeof(Constants); cb.Usage = D3D11_USAGE_DEFAULT; cb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    if (FAILED(device->CreateRasterizerState(&rs, &p->raster)) ||
        FAILED(device->CreateBuffer(&cb, nullptr, &p->constants))) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    *out = p.release(); return RSF_DEPTH_SCALE_OK;
}
catch (...) { return RSF_DEPTH_SCALE_RESOURCE_FAILED; }

extern "C" int32_t rsf_depth_scale_prepare(rsf_depth_scale* p, const rsf_depth_scale_frame* frame) try
{
    if (!p || !frame || frame->struct_size < sizeof(*frame)) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_DEPTH_SCALE_ABI_VERSION) return RSF_DEPTH_SCALE_ABI_MISMATCH;
    if (p->thread != GetCurrentThreadId() || p->original) return RSF_DEPTH_SCALE_BUSY;
    if (!frame->texture || !std::isfinite(frame->jitter_pixels[0]) || !std::isfinite(frame->jitter_pixels[1]) ||
        std::abs(frame->jitter_pixels[0]) > 16384 || std::abs(frame->jitter_pixels[1]) > 16384) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    auto* texture = static_cast<ID3D11Texture2D*>(frame->texture);
    D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
    ComPtr<ID3D11Device> owner; texture->GetDevice(&owner);
    if (owner.Get() != p->device.Get() || d.Format != DXGI_FORMAT_R24G8_TYPELESS || d.MipLevels != 1 ||
        d.ArraySize != 1 || d.SampleDesc.Count != 1 || d.Usage != D3D11_USAGE_DEFAULT ||
        !(d.BindFlags & D3D11_BIND_DEPTH_STENCIL) || !rect_valid(frame->source_rect, d) || !rect_valid(frame->output_rect, d))
        return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    if (!p->snapshot || p->width != d.Width || p->height != d.Height) {
        // Build the complete replacement set before replacing the reusable resources.
        ComPtr<ID3D11Texture2D> snapshot, output;
        ComPtr<ID3D11ShaderResourceView> depth, stencil;
        ComPtr<ID3D11DepthStencilView> target;
        d.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_DEPTH_STENCIL; d.CPUAccessFlags = d.MiscFlags = 0;
        D3D11_SHADER_RESOURCE_VIEW_DESC srv{}; srv.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        srv.Texture2D.MipLevels = 1; srv.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
        D3D11_DEPTH_STENCIL_VIEW_DESC dsv{}; dsv.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; dsv.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
        if (FAILED(p->device->CreateTexture2D(&d, nullptr, &snapshot)) ||
            FAILED(p->device->CreateTexture2D(&d, nullptr, &output)) ||
            FAILED(p->device->CreateShaderResourceView(snapshot.Get(), &srv, &depth))) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
        srv.Format = DXGI_FORMAT_X24_TYPELESS_G8_UINT;
        if (FAILED(p->device->CreateShaderResourceView(snapshot.Get(), &srv, &stencil)) ||
            FAILED(p->device->CreateDepthStencilView(output.Get(), &dsv, &target))) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
        p->snapshot = snapshot; p->output = output; p->depth_view = depth; p->stencil_view = stencil; p->output_view = target;
        p->width = d.Width; p->height = d.Height;
    }
    Bindings bindings(p->context.Get());
    if (!bindings.valid) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    auto* c = p->context.Get(); c->ClearState();
    c->CopyResource(p->snapshot.Get(), texture); c->CopyResource(p->output.Get(), p->snapshot.Get());
    c->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    c->VSSetShader(p->vertex.Get(), nullptr, 0); c->PSSetShader(p->depth_shader.Get(), nullptr, 0);
    c->RSSetState(p->raster.Get());
    const auto* r = frame->output_rect;
    const D3D11_VIEWPORT vp{float(r[0]), float(r[1]), float(r[2] - r[0]), float(r[3] - r[1]), 0, 1};
    c->RSSetViewports(1, &vp); c->OMSetRenderTargets(0, nullptr, p->output_view.Get());
    ID3D11ShaderResourceView* views[]{p->depth_view.Get(), p->stencil_view.Get()}; c->PSSetShaderResources(0, 2, views);
    auto* buffer = p->constants.Get(); c->PSSetConstantBuffers(0, 1, &buffer);
    Constants params{};
    for (unsigned i = 0; i < 4; ++i) { params.source[i] = float(frame->source_rect[i]); params.output[i] = float(r[i]); }
    params.jitter[0] = frame->jitter_pixels[0]; params.jitter[1] = frame->jitter_pixels[1];
    c->UpdateSubresource(buffer, 0, nullptr, &params, 0, 0);
    // The depth draw zeroes stencil only inside the destination viewport.
    c->OMSetDepthStencilState(p->depth_state.Get(), 0); c->Draw(3, 0);
    c->PSSetShader(p->stencil_shader.Get(), nullptr, 0);
    for (unsigned i = 0; i < 8; ++i) {
        params.bit = 1u << i; c->UpdateSubresource(buffer, 0, nullptr, &params, 0, 0);
        c->OMSetDepthStencilState(p->stencil_states[i].Get(), 0xff); c->Draw(3, 0);
    }
    if (FAILED(p->device->GetDeviceRemovedReason())) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    p->original = texture; return RSF_DEPTH_SCALE_OK;
}
catch (...) { return RSF_DEPTH_SCALE_RESOURCE_FAILED; }

extern "C" int32_t rsf_depth_scale_commit(rsf_depth_scale* p)
{
    if (!p) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    if (p->thread != GetCurrentThreadId() || p->committed) return RSF_DEPTH_SCALE_BUSY;
    if (!p->original) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    Bindings bindings(p->context.Get());
    if (!bindings.valid) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    p->context->ClearState(); p->context->CopyResource(p->original.Get(), p->output.Get()); p->committed = true;
    return SUCCEEDED(p->device->GetDeviceRemovedReason()) ? RSF_DEPTH_SCALE_OK : RSF_DEPTH_SCALE_RESOURCE_FAILED;
}

extern "C" int32_t rsf_depth_scale_restore(rsf_depth_scale* p)
{
    if (!p) return RSF_DEPTH_SCALE_INVALID_ARGUMENT;
    if (p->thread != GetCurrentThreadId()) return RSF_DEPTH_SCALE_BUSY;
    if (p->committed) {
        Bindings bindings(p->context.Get());
        if (!bindings.valid) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
        p->context->ClearState(); p->context->CopyResource(p->original.Get(), p->snapshot.Get());
        if (FAILED(p->device->GetDeviceRemovedReason())) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    }
    p->committed = false; p->original.Reset(); return RSF_DEPTH_SCALE_OK;
}

extern "C" int32_t rsf_depth_scale_destroy(rsf_depth_scale* p)
{
    if (!p) return RSF_DEPTH_SCALE_OK;
    const auto result = rsf_depth_scale_restore(p);
    if (result != RSF_DEPTH_SCALE_OK) return result;
    delete p; return RSF_DEPTH_SCALE_OK;
}
