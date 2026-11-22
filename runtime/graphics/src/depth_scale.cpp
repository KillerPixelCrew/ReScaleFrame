// SPDX-License-Identifier: MIT
#include <rescaleframe/depth_scale.h>
#include "depth_platform.h"
#include "depth_scale_private.h"
#include "device_owner.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>

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

void* immediate_device(void* raw)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return nullptr;
    ID3D11Device* device{};
    context->GetDevice(&device);
    return device;
}

int32_t make_pipeline(void* raw, const rsf_depth_scale_setup* setup, uint32_t constant_bytes,
    rsf_depth_scale_pipeline* p) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
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
    D3D11_BUFFER_DESC cb{}; cb.ByteWidth = constant_bytes; cb.Usage = D3D11_USAGE_DEFAULT; cb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    if (FAILED(device->CreateRasterizerState(&rs, &p->raster)) ||
        FAILED(device->CreateBuffer(&cb, nullptr, &p->constants))) return RSF_DEPTH_SCALE_RESOURCE_FAILED;
    return RSF_DEPTH_SCALE_OK;
} catch (...) { return RSF_DEPTH_SCALE_RESOURCE_FAILED; }

int32_t describe(void* device, void* raw, uint32_t* size)
{
    auto* texture = static_cast<ID3D11Texture2D*>(raw);
    D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
    if (!rsf::same_device(texture, static_cast<ID3D11Device*>(device)) ||
        d.Format != DXGI_FORMAT_R24G8_TYPELESS || d.MipLevels != 1 || d.ArraySize != 1 ||
        d.SampleDesc.Count != 1 || d.Usage != D3D11_USAGE_DEFAULT || !(d.BindFlags & D3D11_BIND_DEPTH_STENCIL)) return 0;
    size[0] = d.Width; size[1] = d.Height;
    return 1;
}

int32_t ready(void* device)
{
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->GetDeviceRemovedReason());
}

// Allocation only. It never inspects contents or changes bindings.
int32_t make_surfaces(void* raw, void* source, rsf_depth_scale_surfaces* out) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
    D3D11_TEXTURE2D_DESC d{}; static_cast<ID3D11Texture2D*>(source)->GetDesc(&d);
    ComPtr<ID3D11Texture2D> snapshot, output;
    ComPtr<ID3D11ShaderResourceView> depth, stencil;
    ComPtr<ID3D11DepthStencilView> target;
    d.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_DEPTH_STENCIL; d.CPUAccessFlags = d.MiscFlags = 0;
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{}; srv.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    srv.Texture2D.MipLevels = 1; srv.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    D3D11_DEPTH_STENCIL_VIEW_DESC dsv{}; dsv.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; dsv.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    if (FAILED(device->CreateTexture2D(&d, nullptr, &snapshot)) ||
        FAILED(device->CreateTexture2D(&d, nullptr, &output)) ||
        FAILED(device->CreateShaderResourceView(snapshot.Get(), &srv, &depth))) return 0;
    srv.Format = DXGI_FORMAT_X24_TYPELESS_G8_UINT;
    if (FAILED(device->CreateShaderResourceView(snapshot.Get(), &srv, &stencil)) ||
        FAILED(device->CreateDepthStencilView(output.Get(), &dsv, &target))) return 0;
    *out = {snapshot.Detach(), output.Detach(), depth.Detach(), stencil.Detach(), target.Detach()};
    return 1;
} catch (...) { return 0; }

void clear(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->ClearState();
}

void bind(void* raw, const rsf_depth_scale_pipeline* p, const rsf_depth_scale_surfaces* s, const int32_t* r)
{
    auto* c = static_cast<ID3D11DeviceContext*>(raw);
    c->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    c->VSSetShader(p->vertex, nullptr, 0);
    c->RSSetState(p->raster);
    const D3D11_VIEWPORT vp{float(r[0]), float(r[1]), float(r[2] - r[0]), float(r[3] - r[1]), 0, 1};
    c->RSSetViewports(1, &vp); c->OMSetRenderTargets(0, nullptr, s->output_view);
    ID3D11ShaderResourceView* views[]{s->depth_view, s->stencil_view}; c->PSSetShaderResources(0, 2, views);
    c->PSSetConstantBuffers(0, 1, &p->constants);
}

void draw(void* raw, const rsf_depth_scale_pipeline* p, const void* constants, uint32_t step)
{
    auto* c = static_cast<ID3D11DeviceContext*>(raw);
    // The depth draw zeroes stencil only inside the destination viewport.
    c->PSSetShader(step ? p->stencil_shader : p->depth_shader, nullptr, 0);
    c->UpdateSubresource(p->constants, 0, nullptr, constants, 0, 0);
    if (step) c->OMSetDepthStencilState(p->stencil_states[step - 1], 0xff);
    else c->OMSetDepthStencilState(p->depth_state, 0);
    c->Draw(3, 0);
}

} // namespace

const rsf_depth_scale_platform rsf_depth_scale_native_platform{
    immediate_device, make_pipeline, describe, ready, make_surfaces, rsf_d3d11_state_save,
    rsf_d3d11_state_restore, clear, rsf_d3d11_copy_resource, bind, draw,
};
static_assert(sizeof(rsf_depth_scale_pipeline) == 14 * sizeof(void*));
static_assert(sizeof(rsf_depth_scale_surfaces) == 5 * sizeof(void*));
static_assert(sizeof(rsf_depth_scale_platform) == 11 * sizeof(void*));
static_assert(sizeof(rsf_depth_scale_setup) == 24 && sizeof(rsf_depth_scale_frame) == 56);
static_assert(RSF_DEPTH_SCALE_ABI_VERSION == 1 && RSF_DEPTH_SCALE_OK == 0 &&
    RSF_DEPTH_SCALE_INVALID_ARGUMENT == -1 && RSF_DEPTH_SCALE_ABI_MISMATCH == -2 &&
    RSF_DEPTH_SCALE_RESOURCE_FAILED == -3 && RSF_DEPTH_SCALE_BUSY == -4);

extern "C" int32_t rsf_depth_scale_create(void* context, const rsf_depth_scale_setup* setup, rsf_depth_scale** out)
{
    return rsf_core_depth_scale_create(context, setup, out, &rsf_depth_scale_native_platform);
}

extern "C" int32_t rsf_depth_scale_prepare(rsf_depth_scale* p, const rsf_depth_scale_frame* frame)
{
    return rsf_core_depth_scale_render(p, frame, 1);
}

bool rsf_depth_scale_reserve(rsf_depth_scale* p, ID3D11Texture2D* texture)
{
    return rsf_core_depth_scale_reserve(p, texture) != 0;
}

extern "C" int32_t rsf_depth_scale_commit(rsf_depth_scale* p)
{
    return rsf_core_depth_scale_commit(p);
}

extern "C" int32_t rsf_depth_scale_restore(rsf_depth_scale* p)
{
    return rsf_core_depth_scale_restore(p);
}

extern "C" int32_t rsf_depth_scale_destroy(rsf_depth_scale* p)
{
    return rsf_core_depth_scale_destroy(p);
}

extern "C" int32_t rsf_depth_scale_discard(rsf_depth_scale* p)
{
    return rsf_core_depth_scale_discard(p);
}
