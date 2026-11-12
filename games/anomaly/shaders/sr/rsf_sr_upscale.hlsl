// SPDX-License-Identifier: GPL-3.0-only
// RSF-authored. Anomaly SR Stage 2 seam pass (docs/research/anomaly-sr-seam.md section 10).
//
// One full-screen draw at output size, after the engine's own targets are restored at the S-B seam:
//   SV_Target0  engine UNORM8 generic0  = saturate(f(bilinear(f^-1(y))))  from the render-size FP16 generic0 twin
//   SV_Target1  engine FP16 rt_Position = nearest render-size rt_Position (packed normal, view Z; never filtered)
//   SV_Target2  engine UNORM8 generic1  = bilinear render-size generic1 (distortion mask / bloom source, read by UV)
//   SV_Depth    engine HW.pBaseZB       = nearest render-size depth (stencil is cleared to 0 before the draw)
// y is the engine's "low" tonemap value f(x), f(x) = x(1 + x/W^2)/(1 + x), W = 1.7. The bilinear filter runs on
// x = f^-1(y), the exposure-scaled linear radiance, so it averages light rather than tonemapped values. Stage 5
// replaces this spatial step with temporal SR in the same x space.
//
// Mapping: output pixel p reads source coordinate (p + 0.5) * source / output - 0.5 (texel centres aligned), with
// clamped neighbours. The nearest mapping is floor(p * source / output), the Stage 1 mapping. At scale 1.0 both are
// the identity, so this pass equals the Stage 1 round trip there.

Texture2D<float4> rsf_colour : register(t0);
Texture2D<float4> rsf_position : register(t1);
Texture2D<float4> rsf_distort : register(t2);
Texture2D<float> rsf_depth : register(t3);

cbuffer rsf_upscale : register(b0)
{
    uint2 rsf_output_size;
    uint2 rsf_source_size;
};

static const float rsf_white = 1.7;
static const float rsf_white_squared = rsf_white * rsf_white;

float3 rsf_tonemap_inverse(float3 y)
{
    float3 b = 1.0 - y;
    float3 d = sqrt(b * b + 4.0 * y / rsf_white_squared);
    float3 below = 2.0 * y / max(b + d, 1e-30);
    float3 above = 0.5 * rsf_white_squared * (d - b);
    return b > 0.0 ? below : above;
}

float3 rsf_tonemap_forward(float3 x)
{
    return x * (1.0 + x / rsf_white_squared) / (1.0 + x);
}

// Negative and NaN become 0, +inf the largest finite FP16 value (what the stock UNORM8 store makes of them).
float3 rsf_linear(int2 texel)
{
    float3 y = min(max(rsf_colour.Load(int3(texel, 0)).rgb, 0.0), 65504.0);
    return rsf_tonemap_inverse(y);
}

struct rsf_upscale_output
{
    float4 colour : SV_Target0;
    float4 position : SV_Target1;
    float4 distort : SV_Target2;
    float depth : SV_Depth;
};

float4 rsf_upscale_vs(uint id : SV_VertexID) : SV_Position
{
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}

rsf_upscale_output rsf_upscale_ps(float4 position : SV_Position)
{
    uint2 pixel = uint2(position.xy);
    uint2 output_size = max(rsf_output_size, uint2(1, 1));
    int2 last = int2(rsf_source_size) - 1;
    int2 nearest = int2(min(pixel * rsf_source_size / output_size, rsf_source_size - 1));

    float2 coord = (float2(pixel) + 0.5) * float2(rsf_source_size) / float2(output_size) - 0.5;
    float2 base = floor(coord);
    float2 w = coord - base;
    int2 p0 = clamp(int2(base), int2(0, 0), last);
    int2 p1 = clamp(int2(base) + 1, int2(0, 0), last);

    float3 x = lerp(lerp(rsf_linear(int2(p0.x, p0.y)), rsf_linear(int2(p1.x, p0.y)), w.x),
                    lerp(rsf_linear(int2(p0.x, p1.y)), rsf_linear(int2(p1.x, p1.y)), w.x), w.y);
    float4 d = lerp(lerp(rsf_distort.Load(int3(p0.x, p0.y, 0)), rsf_distort.Load(int3(p1.x, p0.y, 0)), w.x),
                    lerp(rsf_distort.Load(int3(p0.x, p1.y, 0)), rsf_distort.Load(int3(p1.x, p1.y, 0)), w.x), w.y);

    rsf_upscale_output result;
    result.colour = float4(saturate(rsf_tonemap_forward(x)), saturate(rsf_colour.Load(int3(nearest, 0)).a));
    result.position = rsf_position.Load(int3(nearest, 0));
    result.distort = d;
    result.depth = rsf_depth.Load(int3(nearest, 0));
    return result;
}

// ---- Stages 4 and 5 (section 12) ------------------------------------------------------------------------------------
// rsf_prepare_ps, one draw at render size before the SR evaluation:
//   SV_Target0  SR colour   = (f^-1(y), a) of the FP16 generic0 twin: exposure-scaled linear radiance, the space the
//                             Stage 2 filter already works in. Same clamp as rsf_linear.
//   SV_Target1  SR motion   = the motion target (UV x, UV y down, validity, 0), except that an invalid pixel whose
//                             depth is inside the first-person range [0, rsf_first_person_depth) becomes a valid zero
//                             vector when rsf_first_person_zero is set: HUD geometry must not be reconstructed with
//                             the world camera.
// rsf_reinsert_ps, one draw at output size after a successful evaluation: rsf_upscale_ps with the colour taken from the
// reconstructed linear image instead of the bilinear filter, re-tonemapped with the stock curve into UNORM8 generic0.
Texture2D<float4> rsf_reconstructed : register(t4);
Texture2D<float4> rsf_motion : register(t5);

cbuffer rsf_prepare : register(b1)
{
    uint2 rsf_prepare_size;
    float rsf_first_person_depth;
    uint rsf_first_person_zero;
};

struct rsf_prepare_output
{
    float4 colour : SV_Target0;
    float4 motion : SV_Target1;
};

rsf_prepare_output rsf_prepare_ps(float4 position : SV_Position)
{
    int2 pixel = int2(position.xy);
    rsf_prepare_output result;
    float4 y = rsf_colour.Load(int3(pixel, 0));
    result.colour = float4(rsf_linear(pixel), y.a);
    float4 m = rsf_motion.Load(int3(pixel, 0));
    bool valid = m.z > 0.5 && all(isfinite(m.xy));
    float depth = rsf_depth.Load(int3(pixel, 0));
    if (valid)
        result.motion = float4(m.xy, 1.0, 0.0);
    else if (rsf_first_person_zero != 0 && depth < rsf_first_person_depth)
        result.motion = float4(0.0, 0.0, 1.0, 0.0);
    else
        result.motion = float4(0.0, 0.0, 0.0, 0.0);
    return result;
}

rsf_upscale_output rsf_reinsert_ps(float4 position : SV_Position)
{
    rsf_upscale_output result = rsf_upscale_ps(position);
    uint2 pixel = uint2(position.xy);
    float3 x = min(max(rsf_reconstructed.Load(int3(pixel, 0)).rgb, 0.0), 65504.0);
    result.colour.rgb = saturate(rsf_tonemap_forward(x));
    return result;
}
