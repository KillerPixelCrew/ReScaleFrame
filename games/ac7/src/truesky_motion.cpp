// SPDX-License-Identifier: GPL-3.0-only
#include "truesky_motion.h"
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
#include <algorithm>
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
struct Params { uint32_t size[2], divisor[2], grid[2]; float minimum_km, minimum_opacity; };

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
bool prepare(rsf_ac7_cloud_depth& state, ID3D11Device* device, uint32_t width, uint32_t height)
{
    if (state.device != device) { rsf_ac7_cloud_depth_release(state); state.device = device; device->AddRef(); }
    if (state.refused) return false;
    if (!state.shader) {
        state.refused = true;
        const bool compiled = rsf::compile_compute(device, source, "AC7TrueSkyCloudDepth", "main", &state.shader);
        D3D11_BUFFER_DESC buffer{};
        buffer.ByteWidth = sizeof(Params); buffer.Usage = D3D11_USAGE_DEFAULT; buffer.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        if (!compiled || FAILED(device->CreateBuffer(&buffer, nullptr, &state.constants))) return false;
        state.refused = false;
    }
    if (state.texture && state.width == width && state.height == height) return true;
    if (state.view) { state.view->Release(); state.view = nullptr; }
    if (state.texture) { state.texture->Release(); state.texture = nullptr; }
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_R32_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &state.texture)) ||
        FAILED(device->CreateUnorderedAccessView(state.texture, nullptr, &state.view))) {
        if (state.texture) { state.texture->Release(); state.texture = nullptr; }
        return false;
    }
    state.width = width; state.height = height;
    return true;
}
}

ID3D11Texture2D* rsf_ac7_cloud_depth_write(rsf_ac7_cloud_depth& state, ID3D11DeviceContext* context,
    const D3D11_VIEWPORT& viewport)
{
    if (!context || viewport.Width < 16 || viewport.Height < 16 || viewport.Width > 16384 || viewport.Height > 16384) return nullptr;
    ID3D11ShaderResourceView* views[2]{}; context->PSGetShaderResources(1, 2, views);
    ComPtr<ID3D11ShaderResourceView> far_view, near_far_view; far_view.Attach(views[0]); near_far_view.Attach(views[1]);
    ID3D11Buffer* bound = nullptr; context->PSGetConstantBuffers(12, 1, &bound);
    ComPtr<ID3D11Buffer> hdr; hdr.Attach(bound);
    ComPtr<ID3D11Texture2D> far_texture, near_far;
    D3D11_TEXTURE2D_DESC far_desc{}, near_far_desc{};
    if (!hdr || !texture_of(far_view.Get(), far_texture, far_desc) || !texture_of(near_far_view.Get(), near_far, near_far_desc) ||
        far_desc.Width != near_far_desc.Width || far_desc.Height != near_far_desc.Height) return nullptr;
    D3D11_BUFFER_DESC hdr_desc{}; hdr->GetDesc(&hdr_desc);
    if (hdr_desc.ByteWidth < 640) return nullptr;
    const auto width = uint32_t(viewport.Width), height = uint32_t(viewport.Height);
    // TrueSky addresses its grid as (pixel - offset) / divisor. Recover the divisor from extents;
    // the grid carries up to a texel of padding at divisor 1.
    auto divisor = [](uint32_t pixels, uint32_t grid) { return grid ? (std::max)(1u, (pixels + grid / 2) / grid) : 0u; };
    Params params{{width, height}, {divisor(width, near_far_desc.Width), divisor(height, near_far_desc.Height)},
        {near_far_desc.Width, near_far_desc.Height}, 0.3f, 0.5f};
    if (!params.divisor[0] || !params.divisor[1]) return nullptr;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (!prepare(state, device.Get(), width, height)) return nullptr;
    context->UpdateSubresource(state.constants, 0, nullptr, &params, 0, 0);
    // TrueSky's graphics pass stays bound; only compute bindings are borrowed and restored.
    ComPtr<ID3D11ComputeShader> shader; ID3D11ClassInstance* instances[256]{}; UINT instance_count = 256;
    context->CSGetShader(&shader, instances, &instance_count);
    ID3D11ShaderResourceView* saved_views[2]{}; context->CSGetShaderResources(0, 2, saved_views);
    ID3D11UnorderedAccessView* saved_uav = nullptr; context->CSGetUnorderedAccessViews(0, 1, &saved_uav);
    ID3D11Buffer* saved_buffers[2]{}; context->CSGetConstantBuffers(0, 2, saved_buffers);
    ID3D11ShaderResourceView* inputs[]{far_view.Get(), near_far_view.Get()};
    ID3D11Buffer* buffers[]{hdr.Get(), state.constants};
    context->CSSetShader(state.shader, nullptr, 0);
    context->CSSetShaderResources(0, 2, inputs);
    context->CSSetUnorderedAccessViews(0, 1, &state.view, nullptr);
    context->CSSetConstantBuffers(0, 2, buffers);
    context->Dispatch((width + 7) / 8, (height + 7) / 8, 1);
    context->CSSetShader(shader.Get(), instances, instance_count);
    context->CSSetShaderResources(0, 2, saved_views);
    context->CSSetUnorderedAccessViews(0, 1, &saved_uav, nullptr);
    context->CSSetConstantBuffers(0, 2, saved_buffers);
    for (auto* view : saved_views) if (view) view->Release();
    if (saved_uav) saved_uav->Release();
    for (auto* buffer : saved_buffers) if (buffer) buffer->Release();
    for (UINT i = 0; i < instance_count; ++i) if (instances[i]) instances[i]->Release();
    return state.texture;
}

void rsf_ac7_cloud_depth_release(rsf_ac7_cloud_depth& state)
{
    if (state.view) state.view->Release();
    if (state.texture) state.texture->Release();
    if (state.constants) state.constants->Release();
    if (state.shader) state.shader->Release();
    if (state.device) state.device->Release();
    state = {};
}
