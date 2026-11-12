// SPDX-License-Identifier: GPL-3.0-only
// RSF-authored. Anomaly SR Stage 1 reinsertion pass (docs/research/anomaly-sr-seam.md 2.2 and 5).
//
// Input t0: the render-size FP16 twin of $user$generic0 at the S-B seam. It holds the engine's "low" tonemap
// value y = f(x), unclamped, where f(x) = x(1 + x/W^2)/(1 + x), W = 1.7 and x = colour * s_tonemap.
// Output: the engine's own output-size UNORM8 generic0, written as saturate(f(x)), which is what the stock
// UNORM8 target stores for the same y.
//
// Stage 1 runs the exact inverse and then the forward curve with nothing in between, so at scale 1.0 the pass
// is an identity up to FP16 storage and float rounding. Stage 5 replaces the middle with SR output in x.
// Nearest mapping from output to source pixels; at scale 1.0 every output pixel reads its own source pixel.

Texture2D<float4> rsf_source : register(t0);

cbuffer rsf_roundtrip : register(b0)
{
    uint2 rsf_output_size;
    uint2 rsf_source_size;
};

static const float rsf_white = 1.7;
static const float rsf_white_squared = rsf_white * rsf_white;

// f^-1. b = 1 - y, D = sqrt(b^2 + 4y/W^2); x = 2y/(b + D) when b > 0 (no cancellation), else (W^2/2)(D - b).
float3 rsf_tonemap_inverse(float3 y)
{
    float3 b = 1.0 - y;
    float3 d = sqrt(b * b + 4.0 * y / rsf_white_squared);
    float3 below = 2.0 * y / max(b + d, 1e-30);
    float3 above = 0.5 * rsf_white_squared * (d - b);
    return b > 0.0 ? below : above;
}

// f, the stock forward curve.
float3 rsf_tonemap_forward(float3 x)
{
    return x * (1.0 + x / rsf_white_squared) / (1.0 + x);
}

float4 rsf_roundtrip_vs(uint id : SV_VertexID) : SV_Position
{
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}

float4 rsf_roundtrip_ps(float4 position : SV_Position) : SV_Target
{
    uint2 pixel = uint2(position.xy);
    uint2 source = min(pixel * rsf_source_size / max(rsf_output_size, uint2(1, 1)), rsf_source_size - 1);
    float4 value = rsf_source.Load(int3(source, 0));
    // Negative and NaN inputs become 0 and +inf becomes the largest finite FP16 value, which is what the stock
    // UNORM8 conversion makes of them after saturation (min/max return the non-NaN operand).
    float3 y = min(max(value.rgb, 0.0), 65504.0);
    float3 x = rsf_tonemap_inverse(y);
    return float4(saturate(rsf_tonemap_forward(x)), saturate(value.a));
}
