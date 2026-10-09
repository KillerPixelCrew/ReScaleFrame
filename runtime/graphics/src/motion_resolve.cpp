// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/motion_resolve.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_cache.h>
#include <rescaleframe/srv_format.h>
#include "device_owner.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <cmath>
#include <cstring>
#include <new>
using Microsoft::WRL::ComPtr;
namespace {
constexpr char shader_source[] = R"(
Texture2D<float2> Motion : register(t0);
Texture2D<float> Depth : register(t1);
Texture2D<float> DepthLayer : register(t2);
RWTexture2D<float2> Resolved : register(u0);
RWTexture2D<float> ResolvedDepth : register(u1);
cbuffer Params : register(b0) {
    row_major float4x4 ClipToPrevious;
    float2 DecodedToPixels;
    float Sentinel;
    uint HasSentinel;
    uint2 Size;
    uint HasLayer;
    uint Padding;
};
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float depth = Depth[id.xy];
    float2 decoded = Motion[id.xy];
    float2 motion = decoded * DecodedToPixels;
    if (HasSentinel != 0 && all(decoded == Sentinel)) {
        float2 ndc = (float2(id.xy) + 0.5) / float2(Size) * float2(2,-2) + float2(-1,1);
        // Reversed Z: the larger value is the nearer surface, such as a cloud in front of sky.
        float reprojected = HasLayer != 0 ? max(depth, DepthLayer[id.xy]) : depth;
        float4 previous = mul(float4(ndc, reprojected, 1), ClipToPrevious);
        motion = abs(previous.w) > 1e-8 ? (previous.xy / previous.w - ndc) * float2(Size) * float2(0.5,-0.5) : 0;
    }
    Resolved[id.xy] = all(isfinite(motion)) ? motion : 0;
    ResolvedDepth[id.xy] = depth;
})";
struct Constants {
    float matrix[16]; float scale[2]; float sentinel; uint32_t has_sentinel;
    uint32_t size[2]; uint32_t has_layer; uint32_t padding;
};
}
struct rsf_motion_resolve {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11ComputeShader> shader;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11Texture2D> textures[2];
    ComPtr<ID3D11UnorderedAccessView> views[2];
    // Motion, depth and the depth layer are engine targets that come back as the same textures.
    rsf::SrvCache<8> sources;
    uint32_t width = 0, height = 0;
};
extern "C" int rsf_motion_resolve_create(void* pointer, uint32_t width, uint32_t height, rsf_motion_resolve** out)
{
    if (!pointer || !out || !width || !height) return 0;
    *out = nullptr;
    auto* pass = new (std::nothrow) rsf_motion_resolve;
    if (!pass) return 0;
    pass->device = static_cast<ID3D11Device*>(pointer); pass->width = width; pass->height = height;
    if (!rsf::compile_compute(pass->device.Get(), shader_source, "motion_resolve", "main", &pass->shader,
            nullptr, nullptr, 0)) { delete pass; return 0; }
    D3D11_BUFFER_DESC buffer{};
    buffer.ByteWidth = sizeof(Constants); buffer.Usage = D3D11_USAGE_DEFAULT; buffer.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    if (FAILED(pass->device->CreateBuffer(&buffer, nullptr, &pass->constants))) { delete pass; return 0; }
    for (uint32_t i = 0; i < 2; ++i) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        // R16G16_FLOAT, not R32G32_FLOAT: this output crosses to D3D12 for DLSS-G, FSR and XeSS,
        // and R32G32_FLOAT is not a D3D11 shareable format (CreateTexture2D E_INVALIDARG on the
        // RTX 4070 Laptop, 4 October). The R32G32 decode upstream keeps the NDC precision.
        desc.Format = i == 0 ? DXGI_FORMAT_R16G16_FLOAT : DXGI_FORMAT_R32_FLOAT;
        desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(pass->device->CreateTexture2D(&desc, nullptr, &pass->textures[i])) ||
            FAILED(pass->device->CreateUnorderedAccessView(pass->textures[i].Get(), nullptr, &pass->views[i]))) {
            delete pass; return 0;
        }
    }
    *out = pass; return 1;
}
extern "C" int rsf_motion_resolve_run(rsf_motion_resolve* pass, void* context_pointer,
    void* motion_pointer, void* depth_pointer, const rsf_motion_resolve_params* params)
{
    if (!pass || !context_pointer || !motion_pointer || !depth_pointer || !params || params->struct_size < sizeof(*params)) return 0;
    for (auto value : params->clip_to_previous) if (!std::isfinite(value)) return 0;
    if (!std::isfinite(params->decoded_to_pixels[0]) || !std::isfinite(params->decoded_to_pixels[1]) ||
        !std::isfinite(params->sentinel)) return 0;
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    if (!rsf::same_device(context, pass->device.Get()) || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return 0;
    ID3D11Texture2D* inputs[] = {static_cast<ID3D11Texture2D*>(motion_pointer), static_cast<ID3D11Texture2D*>(depth_pointer)};
    ID3D11ShaderResourceView* source_views[2]{};
    for (uint32_t i = 0; i < 2; ++i) {
        D3D11_TEXTURE2D_DESC desc{}; inputs[i]->GetDesc(&desc);
        if (!rsf::same_device(inputs[i], pass->device.Get()) || desc.Width < pass->width || desc.Height < pass->height ||
            desc.SampleDesc.Count != 1 || desc.ArraySize != 1) return 0;
        source_views[i] = pass->sources.get(pass->device.Get(), inputs[i], rsf::srv_format(desc.Format));
        if (!source_views[i]) return 0;
    }
    ID3D11ShaderResourceView* layer_view = nullptr;
    if (params->struct_size >= sizeof(*params) && params->depth_layer) {
        auto* layer = static_cast<ID3D11Texture2D*>(params->depth_layer);
        D3D11_TEXTURE2D_DESC desc{}; layer->GetDesc(&desc);
        // A layer that does not fit is ignored: scene depth alone is the established behaviour.
        if (rsf::same_device(layer, pass->device.Get()) && desc.Format == DXGI_FORMAT_R32_FLOAT && desc.Width >= pass->width &&
            desc.Height >= pass->height && desc.SampleDesc.Count == 1 && desc.ArraySize == 1)
            layer_view = pass->sources.get(pass->device.Get(), layer, DXGI_FORMAT_R32_FLOAT);
    }
    Constants constants{};
    constants.has_layer = layer_view ? 1u : 0u;
    std::memcpy(constants.matrix, params->clip_to_previous, sizeof(constants.matrix));
    std::memcpy(constants.scale, params->decoded_to_pixels, sizeof(constants.scale));
    constants.sentinel = params->sentinel; constants.has_sentinel = params->has_sentinel;
    constants.size[0] = pass->width; constants.size[1] = pass->height;
    context->UpdateSubresource(pass->constants.Get(), 0, nullptr, &constants, 0, 0);
    rsf_d3d11_state saved{};
    if (!rsf_d3d11_state_save(context, &saved)) return 0;
    context->SetPredication(nullptr, FALSE);
    context->OMSetRenderTargets(0, nullptr, nullptr);
    ID3D11ShaderResourceView* sources[] = {source_views[0], source_views[1], layer_view};
    ID3D11UnorderedAccessView* targets[] = {pass->views[0].Get(), pass->views[1].Get()};
    ID3D11Buffer* buffers[] = {pass->constants.Get()};
    context->CSSetShader(pass->shader.Get(), nullptr, 0); context->CSSetShaderResources(0, 3, sources);
    context->CSSetUnorderedAccessViews(0, 2, targets, nullptr); context->CSSetConstantBuffers(0, 1, buffers);
    context->Dispatch((pass->width + 7) / 8, (pass->height + 7) / 8, 1);
    ID3D11UnorderedAccessView* empty[2]{}; context->CSSetUnorderedAccessViews(0, 2, empty, nullptr);
    rsf_d3d11_state_restore(context, &saved);
    return 1;
}
extern "C" void* rsf_motion_resolve_motion(rsf_motion_resolve* pass) { return pass ? pass->textures[0].Get() : nullptr; }
extern "C" void* rsf_motion_resolve_depth(rsf_motion_resolve* pass) { return pass ? pass->textures[1].Get() : nullptr; }
extern "C" void rsf_motion_resolve_destroy(rsf_motion_resolve* pass) { delete pass; }
