// SPDX-License-Identifier: MIT
#include "native_regions.h"
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>

extern "C" {
using CompileShader = void* (*)(void*, uint32_t);
uint32_t rsf_core_native_regions_depth_valid(const rsf_game_depth_domains*, uint32_t format, const int32_t rect[4]);
uint32_t rsf_core_native_regions_prepare(void* context, void* const inputs[3], const int32_t rect[4],
    const rsf_game_depth_domains*, uint32_t inverted, CompileShader, void* outputs[3]);
uint32_t rsf_core_native_regions_temporal_mask(void* context, void* const inputs[3], const int32_t rect[4],
    const float weights[4], CompileShader, void* outputs[2]);
void rsf_core_native_regions_release();
}

namespace {
constexpr char depth_shader[] = R"(
Texture2D<float> Source : register(t0);
RWTexture2D<float> Target : register(u0);
struct Domain { float4 Range; float4 Viewport; row_major float4x4 Transform; uint4 Flags; };
cbuffer Region : register(b0) {
    uint2 Origin; uint2 Size;
    uint Count; uint ClearEnabled; float ClearValue; float FarClear;
    Domain Domains[2];
};
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float stored = Source.Load(int3(Origin + id.xy, 0));
    if (Count == 0) { Target[id.xy] = stored; return; }
    if (ClearEnabled != 0 && stored == ClearValue) { Target[id.xy] = FarClear; return; }
    // CPU validation proves exhaustive, unambiguous UNORM coverage and finite positive W.
    uint selected = 0;
    if (Count == 2) {
        bool upper = stored < Domains[0].Range.y ||
            (stored == Domains[0].Range.y && (Domains[0].Flags.x & 2) != 0);
        selected = upper ? 0 : 1;
    }
    Domain d = Domains[selected];
    float2 xy = (float2(Origin + id.xy) + 0.5 - d.Viewport.xy) * d.Viewport.zw;
    float z = (stored - d.Range.z) * d.Range.w;
    float4 clip = mul(float4(xy * float2(2,-2) + float2(-1,1), z, 1), d.Transform);
    // Depth-only clipping at the existing composed sample, not spatial reprojection.
    Target[id.xy] = saturate(clip.z / clip.w);
})";
constexpr char color_shader[] = R"(
Texture2D<float3> Source : register(t0);
RWTexture2D<float4> Target : register(u0);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) Target[id.xy] = float4(Source.Load(int3(Origin + id.xy, 0)), 1);
})";
constexpr char temporal_mask_shader[] = R"(
Texture2D<float4> Source : register(t0);
Texture2D<float> Reactive : register(t1);
Texture2D<float> Bias : register(t2);
RWTexture2D<float> OutReactive : register(u0);
RWTexture2D<float> OutBias : register(u1);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; float4 Weights; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) {
        float hint = dot(Source.Load(int3(Origin + id.xy, 0)), Weights);
        hint = isfinite(hint) ? saturate(hint) : 1;
        OutReactive[id.xy] = max(hint, Reactive.Load(int3(id.xy, 0)));
        OutBias[id.xy] = max(hint, Bias.Load(int3(id.xy, 0)));
    }
})";

static_assert(sizeof(rsf_game_depth_domain) == 104 && sizeof(rsf_game_depth_domains) == 240,
    "Rust depth-domain ABI");

struct ReleaseAtUnload {
    ~ReleaseAtUnload() { rsf_core_native_regions_release(); }
} release_at_unload;
}

extern "C" {
static void* compile_regions_shader(void* device, uint32_t kernel) try
{
    constexpr const char* sources[]{depth_shader, color_shader, temporal_mask_shader};
    constexpr const char* names[]{"NativeDepthRegion", "NativeColorRegion", "NativeTemporalMaskRegion"};
    if (kernel >= 3) return nullptr;
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> result;
    if (!rsf::compile_compute(static_cast<ID3D11Device*>(device), sources[kernel], names[kernel],
            "main", result.GetAddressOf())) return nullptr;
    return result.Detach();
}
catch (...) { return nullptr; }
}

bool rsf_native_regions_depth_valid(const rsf_game_depth_domains* domains, DXGI_FORMAT format, const int32_t rect[4])
{
    return rsf_core_native_regions_depth_valid(domains, uint32_t(format), rect) != 0;
}

bool rsf_native_regions_prepare(ID3D11DeviceContext* context, ID3D11Texture2D* color,
    ID3D11Texture2D* depth, ID3D11Texture2D* motion, const int32_t rect[4],
    ID3D11Texture2D** out_color, ID3D11Texture2D** out_depth, ID3D11Texture2D** out_motion,
    const rsf_game_depth_domains* domains, bool inverted)
{
    if (!out_color || !out_depth || !out_motion) return false;
    void* inputs[]{color, depth, motion};
    void* outputs[3]{};
    if (!rsf_core_native_regions_prepare(context, inputs, rect, domains, uint32_t(inverted), compile_regions_shader, outputs)) return false;
    *out_color = static_cast<ID3D11Texture2D*>(outputs[0]);
    *out_depth = static_cast<ID3D11Texture2D*>(outputs[1]);
    *out_motion = static_cast<ID3D11Texture2D*>(outputs[2]);
    return true;
}

bool rsf_native_regions_temporal_mask(ID3D11DeviceContext* context, ID3D11Texture2D* mask,
    const int32_t rect[4], const float weights[4], ID3D11Texture2D* reactive, ID3D11Texture2D* bias,
    ID3D11Texture2D** out_reactive, ID3D11Texture2D** out_bias)
{
    if (!out_reactive || !out_bias) return false;
    void* inputs[]{mask, reactive, bias};
    void* outputs[2]{};
    if (!rsf_core_native_regions_temporal_mask(context, inputs, rect, weights, compile_regions_shader, outputs)) return false;
    *out_reactive = static_cast<ID3D11Texture2D*>(outputs[0]);
    *out_bias = static_cast<ID3D11Texture2D*>(outputs[1]);
    return true;
}

void rsf_native_regions_release() { rsf_core_native_regions_release(); }
