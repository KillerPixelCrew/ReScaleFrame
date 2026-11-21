// SPDX-License-Identifier: MIT
#include "truesky_motion.h"
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
using Microsoft::WRL::ComPtr;

namespace {
// HdrConstants offsets measured in F9 captures of composite_tile (PS CRC32C 83524e47):
// tanHalfFov at 448, depthToLinFadeDistParams at 464, maxFadeDistanceKm at 580,
// maxCloudDistanceKm at 636. The composite converts device depth to normalized radial distance
// with sqrt(1+|ndc*tanHalfFov|^2) * x/(d*y+z) (+ d*w); with z = w = 0, as captured, this inverts
// to d = x / (y * planar). The near/far texture's z channel is km along the ray to the first 25%
// opacity, 150 (maxFade) when the ray never reaches it and 100 (maxCloud) when it misses the layer.
constexpr char source[] = R"(
Texture2D<float4> Far : register(t0);
Texture2D<float4> NearFar : register(t1);
RWTexture2D<float> CloudDepth : register(u0);
cbuffer HdrConstants : register(b0) { float4 Hdr[40]; };
cbuffer Params : register(b1) {
    uint2 Size;
    uint2 Divisor;
    uint2 GridSize;
    float MinimumKm;
    float MinimumOpacity;
};
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    uint2 texel = min(id.xy / Divisor, GridSize - 1);
    float4 distances = NearFar.Load(int3(texel, 0));
    float transmittance = Far.Load(int3(texel, 0)).a;
    float2 tanHalfFov = Hdr[28].xy;
    float4 depthToLinear = Hdr[29];
    float maxFadeKm = Hdr[36].y, maxCloudKm = Hdr[39].w;
    float km = distances.z;
    float depth = 0;
    if (depthToLinear.x > 0 && depthToLinear.y > 0 && maxFadeKm > 0 && maxCloudKm > 0 &&
        km < maxCloudKm && km < maxFadeKm && distances.w > 0 && 1 - transmittance >= MinimumOpacity) {
        float2 ndc = (float2(id.xy) + 0.5) / float2(Size) * 2 - 1;
        float2 ray = ndc * tanHalfFov;
        float planar = max(km, MinimumKm) / maxFadeKm / sqrt(1 + dot(ray, ray));
        depth = depthToLinear.x / (depthToLinear.y * planar);
    }
    CloudDepth[id.xy] = isfinite(depth) ? depth : 0;
})";

bool texture_of(ID3D11ShaderResourceView* view, ComPtr<ID3D11Texture2D>& texture, D3D11_TEXTURE2D_DESC& desc)
{
    if (!view) return false;
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{}; view->GetDesc(&srv);
    if (srv.ViewDimension != D3D11_SRV_DIMENSION_TEXTURE2D || srv.Format != DXGI_FORMAT_R16G16B16A16_FLOAT) return false;
    ComPtr<ID3D11Resource> resource; view->GetResource(&resource);
    if (!resource || FAILED(resource.As(&texture))) return false;
    texture->GetDesc(&desc);
    return desc.SampleDesc.Count == 1 && desc.ArraySize == 1;
}
bool create_pipeline(void* owning_device, void** shader, void** constants) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(owning_device);
    ComPtr<ID3D11ComputeShader> compiled;
    if (!rsf::compile_compute(device, source, "AC7TrueSkyCloudDepth", "main", compiled.GetAddressOf()))
        return false;
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = sizeof(rsf_ac7_cloud_depth_params);
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    ComPtr<ID3D11Buffer> buffer;
    if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return false;
    *shader = compiled.Detach();
    *constants = buffer.Detach();
    return true;
}
catch (...) { return false; }

bool create_target(void* owning_device, uint32_t width, uint32_t height, void** texture, void** view) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(owning_device);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_R32_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    ComPtr<ID3D11Texture2D> target;
    ComPtr<ID3D11UnorderedAccessView> uav;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &target)) ||
        FAILED(device->CreateUnorderedAccessView(target.Get(), nullptr, &uav))) return false;
    *texture = target.Detach();
    *view = uav.Detach();
    return true;
}
catch (...) { return false; }

constexpr rsf_ac7_cloud_depth_leaves leaves{
    [](void* object) noexcept { static_cast<IUnknown*>(object)->AddRef(); },
    [](void* object) noexcept { static_cast<IUnknown*>(object)->Release(); },
    create_pipeline,
    create_target,
};
}

ID3D11Texture2D* rsf_ac7_cloud_depth_write(rsf_ac7_cloud_depth& state, ID3D11DeviceContext* context,
    const D3D11_VIEWPORT& viewport)
{
    if (!context) return nullptr;
    ID3D11ShaderResourceView* views[2]{}; context->PSGetShaderResources(1, 2, views);
    ComPtr<ID3D11ShaderResourceView> far_view, near_far_view; far_view.Attach(views[0]); near_far_view.Attach(views[1]);
    ID3D11Buffer* bound = nullptr; context->PSGetConstantBuffers(12, 1, &bound);
    ComPtr<ID3D11Buffer> hdr; hdr.Attach(bound);
    ComPtr<ID3D11Texture2D> far_texture, near_far;
    D3D11_TEXTURE2D_DESC far_desc{}, near_far_desc{};
    if (!hdr || !texture_of(far_view.Get(), far_texture, far_desc) ||
        !texture_of(near_far_view.Get(), near_far, near_far_desc)) return nullptr;
    D3D11_BUFFER_DESC hdr_desc{}; hdr->GetDesc(&hdr_desc);
    const rsf_ac7_cloud_depth_facts facts{viewport.Width, viewport.Height, far_desc.Width, far_desc.Height,
        near_far_desc.Width, near_far_desc.Height, hdr_desc.ByteWidth};
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    rsf_ac7_cloud_depth_params params{};
    if (!rsf_core_ac7_cloud_depth_prepare(&state, device.Get(), &facts, &params, &leaves)) return nullptr;
    auto* constants = static_cast<ID3D11Buffer*>(state.constants);
    auto* output = static_cast<ID3D11UnorderedAccessView*>(state.view);
    context->UpdateSubresource(constants, 0, nullptr, &params, 0, 0);
    // TrueSky's graphics pass stays bound; only compute bindings are borrowed and restored.
    ComPtr<ID3D11ComputeShader> shader; ID3D11ClassInstance* instances[256]{}; UINT instance_count = 256;
    context->CSGetShader(&shader, instances, &instance_count);
    ID3D11ShaderResourceView* saved_views[2]{}; context->CSGetShaderResources(0, 2, saved_views);
    ID3D11UnorderedAccessView* saved_uav = nullptr; context->CSGetUnorderedAccessViews(0, 1, &saved_uav);
    ID3D11Buffer* saved_buffers[2]{}; context->CSGetConstantBuffers(0, 2, saved_buffers);
    ID3D11ShaderResourceView* inputs[]{far_view.Get(), near_far_view.Get()};
    ID3D11Buffer* buffers[]{hdr.Get(), constants};
    context->CSSetShader(static_cast<ID3D11ComputeShader*>(state.shader), nullptr, 0);
    context->CSSetShaderResources(0, 2, inputs);
    context->CSSetUnorderedAccessViews(0, 1, &output, nullptr);
    context->CSSetConstantBuffers(0, 2, buffers);
    context->Dispatch((params.size[0] + 7) / 8, (params.size[1] + 7) / 8, 1);
    context->CSSetShader(shader.Get(), instances, instance_count);
    context->CSSetShaderResources(0, 2, saved_views);
    context->CSSetUnorderedAccessViews(0, 1, &saved_uav, nullptr);
    context->CSSetConstantBuffers(0, 2, saved_buffers);
    for (auto* view : saved_views) if (view) view->Release();
    if (saved_uav) saved_uav->Release();
    for (auto* buffer : saved_buffers) if (buffer) buffer->Release();
    for (UINT i = 0; i < instance_count; ++i) if (instances[i]) instances[i]->Release();
    return static_cast<ID3D11Texture2D*>(state.texture);
}

void rsf_ac7_cloud_depth_release(rsf_ac7_cloud_depth& state)
{
    rsf_core_ac7_cloud_depth_release(&state, &leaves);
}
