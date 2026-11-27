// SPDX-License-Identifier: MIT
// FidelityFX Super Resolution 1.0 edge-adaptive spatial upsampling (EASU), the 32-bit path of
// FsrEasuF in AMD's ffx_fsr1.h (Copyright (c) 2021 Advanced Micro Devices, Inc., MIT). This
// version loads the twelve taps instead of gathering them, works in a tonemapped gamma-2 space
// like Unity's FSR1 pass, and samples the source at its jitter-free position: the engine drew the
// scene content of position q at q - jitter, as the auxiliary scaler also reads it.
Texture2D<float4> source : register(t0);
RWTexture2D<float4> target : register(u0);
cbuffer Constants : register(b0) { uint2 input; uint2 output; float2 jitter; };

float3 Perceptual(float3 c) {
    c = max(c, 0.0);
    return sqrt(c * rcp(1.0 + max(c.r, max(c.g, c.b))));
}

float3 Linear(float3 c) {
    c = min(c * c, 0.9999);
    return c * rcp(1.0 - max(c.r, max(c.g, c.b)));
}

float3 Tap(int2 p) {
    return Perceptual(source.Load(int3(clamp(p, int2(0, 0), int2(input) - 1), 0)).rgb);
}

// Luma times two in two multiply-adds, as AMD's.
float Luma(float3 c) { return c.b * 0.5 + (c.r * 0.5 + c.g); }
float RcpLo(float a) { return asfloat(0x7ef07ebbu - asuint(a)); }
float RsqLo(float a) { return asfloat(0x5f347d74u - (asuint(a) >> 1)); }

// One bilinear corner's edge direction and length from its plus-shaped neighbourhood.
void Direction(inout float2 dir, inout float len, float w,
    float lA, float lB, float lC, float lD, float lE) {
    float lenX = RcpLo(max(abs(lD - lC), abs(lC - lB)));
    float dirX = lD - lB;
    dir.x += dirX * w;
    lenX = saturate(abs(dirX) * lenX);
    len += lenX * lenX * w;
    float lenY = RcpLo(max(abs(lE - lC), abs(lC - lA)));
    float dirY = lE - lA;
    dir.y += dirY * w;
    lenY = saturate(abs(dirY) * lenY);
    len += lenY * lenY * w;
}

void Accumulate(inout float3 color, inout float weight, float2 offset, float2 dir, float2 len,
    float lob, float clp, float3 c) {
    float2 v = float2(offset.x * dir.x + offset.y * dir.y, offset.x * -dir.y + offset.y * dir.x);
    v *= len;
    float d2 = min(v.x * v.x + v.y * v.y, clp);
    float wB = 0.4 * d2 - 1.0;
    float wA = lob * d2 - 1.0;
    wB *= wB;
    wA *= wA;
    wB = 1.5625 * wB - 0.5625;
    float w = wB * wA;
    color += c * w;
    weight += w;
}

[numthreads(8, 8, 1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= output)) return;
    float2 pp = (float2(id.xy) + 0.5) * (float2(input) / float2(output)) - 0.5 - jitter;
    float2 fp = floor(pp);
    pp -= fp;
    int2 o = int2(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    float3 b = Tap(o + int2(0, -1)), c = Tap(o + int2(1, -1));
    float3 e = Tap(o + int2(-1, 0)), f = Tap(o), g = Tap(o + int2(1, 0)), h = Tap(o + int2(2, 0));
    float3 i = Tap(o + int2(-1, 1)), j = Tap(o + int2(0, 1)), k = Tap(o + int2(1, 1));
    float3 l = Tap(o + int2(2, 1)), n = Tap(o + int2(0, 2)), q = Tap(o + int2(1, 2));
    float bL = Luma(b), cL = Luma(c), eL = Luma(e), fL = Luma(f), gL = Luma(g), hL = Luma(h);
    float iL = Luma(i), jL = Luma(j), kL = Luma(k), lL = Luma(l), nL = Luma(n), oL = Luma(q);
    float2 dir = 0.0;
    float len = 0.0;
    Direction(dir, len, (1.0 - pp.x) * (1.0 - pp.y), bL, eL, fL, gL, jL);
    Direction(dir, len, pp.x * (1.0 - pp.y), cL, fL, gL, hL, kL);
    Direction(dir, len, (1.0 - pp.x) * pp.y, fL, iL, jL, kL, nL);
    Direction(dir, len, pp.x * pp.y, gL, jL, kL, lL, oL);
    float dirR = dir.x * dir.x + dir.y * dir.y;
    bool zero = dirR < 1.0 / 32768.0;
    dirR = zero ? 1.0 : RsqLo(dirR);
    dir.x = zero ? 1.0 : dir.x;
    dir *= dirR;
    len *= 0.5;
    len *= len;
    float stretch = (dir.x * dir.x + dir.y * dir.y) * RcpLo(max(abs(dir.x), abs(dir.y)));
    float2 len2 = float2(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    float lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    float clp = RcpLo(lob);
    float3 color = 0.0;
    float weight = 0.0;
    Accumulate(color, weight, float2(0.0, -1.0) - pp, dir, len2, lob, clp, b);
    Accumulate(color, weight, float2(1.0, -1.0) - pp, dir, len2, lob, clp, c);
    Accumulate(color, weight, float2(-1.0, 1.0) - pp, dir, len2, lob, clp, i);
    Accumulate(color, weight, float2(0.0, 1.0) - pp, dir, len2, lob, clp, j);
    Accumulate(color, weight, float2(0.0, 0.0) - pp, dir, len2, lob, clp, f);
    Accumulate(color, weight, float2(-1.0, 0.0) - pp, dir, len2, lob, clp, e);
    Accumulate(color, weight, float2(1.0, 1.0) - pp, dir, len2, lob, clp, k);
    Accumulate(color, weight, float2(2.0, 1.0) - pp, dir, len2, lob, clp, l);
    Accumulate(color, weight, float2(2.0, 0.0) - pp, dir, len2, lob, clp, h);
    Accumulate(color, weight, float2(1.0, 0.0) - pp, dir, len2, lob, clp, g);
    Accumulate(color, weight, float2(1.0, 2.0) - pp, dir, len2, lob, clp, q);
    Accumulate(color, weight, float2(0.0, 2.0) - pp, dir, len2, lob, clp, n);
    // Deringing: no result outside the four nearest texels.
    float3 low = min(min(f, g), min(j, k));
    float3 high = max(max(f, g), max(j, k));
    target[id.xy] = float4(Linear(min(high, max(low, color * rcp(weight)))), 1.0);
}
