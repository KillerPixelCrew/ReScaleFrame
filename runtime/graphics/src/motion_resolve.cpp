// SPDX-License-Identifier: MIT
#include "motion_resolve_platform.h"
#include <rescaleframe/shader_compile.h>

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
    float2 JitterPixels;
    float2 Padding2;
};
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float depth = Depth[id.xy];
    float2 decoded = Motion[id.xy];
    float2 motion = decoded * DecodedToPixels;
    if (HasSentinel != 0 && all(decoded == Sentinel)) {
        // Depth belongs to a raster sample shifted by the current projection. Both matrices and
        // written vectors exclude jitter, so reconstruct at the corresponding unjittered position.
        float2 ndc = (float2(id.xy) + 0.5 - JitterPixels) / float2(Size) * float2(2,-2) + float2(-1,1);
        // Reversed Z: the larger value is the nearer surface, such as a cloud in front of sky.
        float reprojected = HasLayer != 0 ? max(depth, DepthLayer[id.xy]) : depth;
        float4 previous = mul(float4(ndc, reprojected, 1), ClipToPrevious);
        motion = abs(previous.w) > 1e-8 ? (previous.xy / previous.w - ndc) * float2(Size) * float2(0.5,-0.5) : 0;
    }
    Resolved[id.xy] = all(isfinite(motion)) ? motion : 0;
    ResolvedDepth[id.xy] = depth;
})";
int32_t make_resources(void* raw, uint32_t width, uint32_t height, uint32_t bytes,
    rsf_motion_resolve_resources* resources) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
    if (!rsf::compile_compute(device, shader_source, "motion_resolve", "main",
            &resources->shader, nullptr, nullptr, 0)) return 0;
    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = bytes;
    constants.Usage = D3D11_USAGE_DEFAULT;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    if (FAILED(device->CreateBuffer(&constants, nullptr, &resources->constants))) return 0;
    for (uint32_t i = 0; i < 2; ++i) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = width;
        desc.Height = height;
        desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        // R16G16_FLOAT can cross into D3D12 for FG. Decode upstream
        // retains its separate R32G32_FLOAT format and NDC precision.
        desc.Format = i == 0 ? DXGI_FORMAT_R16G16_FLOAT : DXGI_FORMAT_R32_FLOAT;
        desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(device->CreateTexture2D(&desc, nullptr, &resources->textures[i])) ||
            !resources->textures[i] ||
            FAILED(device->CreateUnorderedAccessView(resources->textures[i], nullptr, &resources->views[i])) ||
            !resources->views[i]) {
            return 0;
        }
    }
    return 1;
} catch (...) { return 0; }

int32_t dispatch(void* raw, const rsf_motion_resolve_resources* resources, void* const* sources,
    const void* constants, uint32_t groups_x, uint32_t groups_y) noexcept try
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    context->UpdateSubresource(resources->constants, 0, nullptr, constants, 0, 0);
    ID3D11ShaderResourceView* inputs[]{
        static_cast<ID3D11ShaderResourceView*>(sources[0]),
        static_cast<ID3D11ShaderResourceView*>(sources[1]),
        static_cast<ID3D11ShaderResourceView*>(sources[2]),
    };
    return rsf::motion11::dispatch(context, resources->shader, resources->constants,
        inputs, resources->views, nullptr, groups_x, groups_y);
} catch (...) { return 0; }

constexpr rsf_motion_resolve_platform platform{
    make_resources, rsf::motion11::context_matches, rsf::motion11::describe_source,
    rsf::motion11::source_view, dispatch,
};

static_assert(sizeof(rsf_motion_resolve_resources) == 6 * sizeof(void*));
static_assert(sizeof(rsf_motion_resolve_platform) == 5 * sizeof(void*) && sizeof(rsf_motion_source_info) == 24);
static_assert(sizeof(rsf_motion_resolve_params) == 104 && offsetof(rsf_motion_resolve_params, jitter_pixels) == 96);
static_assert(DXGI_FORMAT_R32_FLOAT == 41);
}

extern "C" int rsf_motion_resolve_create(void* device, uint32_t width, uint32_t height, rsf_motion_resolve** out)
{
    return rsf_core_motion_resolve_create(device, width, height, out, &platform);
}

extern "C" int rsf_motion_resolve_run(rsf_motion_resolve* pass, void* context,
    void* motion, void* depth, const rsf_motion_resolve_params* params)
{
    return rsf_core_motion_resolve_run(pass, context, motion, depth, params);
}

extern "C" void* rsf_motion_resolve_motion(rsf_motion_resolve* pass)
{
    return rsf_core_motion_resolve_texture(pass, 0);
}

extern "C" void* rsf_motion_resolve_depth(rsf_motion_resolve* pass)
{
    return rsf_core_motion_resolve_texture(pass, 1);
}

extern "C" void rsf_motion_resolve_destroy(rsf_motion_resolve* pass)
{
    rsf_core_motion_resolve_destroy(pass);
}
