// SPDX-License-Identifier: GPL-3.0-only
// Ported from OptiScaler 2fff542 shaders/rcas/precompile (GPL-3.0).
// RCAS derives from RdenBlaauwen/RCAS-for-ReShade and AMD FidelityFX (MIT).
// See THIRD_PARTY.md for provenance and license terms.
#pragma once
namespace rsf::sharpening {
inline constexpr char rcas[] = R"RSF(
cbuffer Params : register(b0) {
    float Sharpness; float Contrast; uint DynamicSharpenEnabled; uint DisplaySizeMV;
    uint Debug; uint MotionWidth; uint MotionHeight; float MotionSharpness;
    float2 MotionTextureScale; float MvScaleX; float MvScaleY;
    float MotionThreshold; float MotionScaleLimit; uint DisplayWidth; uint DisplayHeight;
    uint DepthIsLinear; uint DepthIsReversed; float DepthScale; float DepthBias;
    float DepthLinearA; float DepthLinearB; float DepthLinearC; uint ClampOutput;
    float2 DepthTextureScale; uint DepthWidth; uint DepthHeight;
    uint EncodeHdr; uint HasExposure; uint KnownPlanes; float ExposureScale;
};
Texture2D<float4> Source : register(t0);
Texture2D<float2> Motion : register(t1);
Texture2D<float> DepthTex : register(t2);
Texture2D<float> Exposure : register(t3);
RWTexture2D<float4> Dest : register(u0);
float exposureValue() {
    return HasExposure != 0 ? max(Exposure.Load(int3(0,0,0)),1e-6) : max(ExposureScale,1e-6);
}
float3 encodeColor(float3 c) {
    if (EncodeHdr == 0) return c;
    float3 x = max(c,0) * exposureValue();
    return pow(x / (x + 4),1.0/2.2);
}
float3 loadColor(int2 p) { return encodeColor(Source.Load(int3(p,0)).rgb); }
float4 storeColor(int2 p, float3 c) {
    float4 original = Source.Load(int3(p,0));
    // A constant image returns its original bits, including negative/extended SDR values.
    float3 center = encodeColor(original.rgb);
    if (ClampOutput == 0 && all(c == center)) return original;
    if (EncodeHdr != 0) {
        // Clamp to the original local range before inverting the bounded curve. This
        // prevents HDR halos and avoids evaluating the inverse at or above one.
        float3 lo = center, hi = center;
        [unroll] for (int y=-1;y<=1;++y) [unroll] for (int x=-1;x<=1;++x) {
            int2 q = clamp(p+int2(x,y),int2(0,0),int2(DisplayWidth,DisplayHeight)-1);
            float3 v = loadColor(q); lo = min(lo,v); hi = max(hi,v);
        }
        if (Debug == 0 && ClampOutput == 0 && all(lo == hi)) return original;
        if (Debug == 0) c = clamp(c,lo,hi);
        float3 t = pow(clamp(c,0,0.999999),2.2);
        c = 4*t / max(1-t,1e-7) / exposureValue();
    }
    if (ClampOutput != 0) c = saturate(c);
    return float4(c,original.a);
}
[numthreads(16, 16, 1)]
void CSMain(uint3 DTid : SV_DispatchThreadID)
{
    // Guard against oversized dispatch
    if ((int) DTid.x >= DisplayWidth || (int) DTid.y >= DisplayHeight)
        return;

    int2 pixel = int2(DTid.xy);
    int2 maxPixel = int2(DisplayWidth - 1, DisplayHeight - 1);

    float setSharpness = Sharpness;

    if (DynamicSharpenEnabled > 0)
    {
        float2 mv = float2(0.0, 0.0);
        float motion = 0.0;
        float add = 0.0;

        int2 mvCoord;
        if (DisplaySizeMV > 0)
        {
            mvCoord = pixel;
        }
        else
        {
            mvCoord = int2(pixel * MotionTextureScale);
        }

        // Clamp motion texture reads to valid range
        mvCoord.x = clamp(mvCoord.x, 0, int(MotionWidth)-1);
        mvCoord.y = clamp(mvCoord.y, 0, int(MotionHeight)-1);

        mv = Motion.Load(int3(mvCoord, 0)).rg;

        motion = max(abs(mv.x * MvScaleX), abs(mv.y * MvScaleY));

        if (motion > MotionThreshold && MotionScaleLimit > MotionThreshold)
        {
            add = ((motion - MotionThreshold) / (MotionScaleLimit - MotionThreshold)) * MotionSharpness;
        }

        if ((add > MotionSharpness && MotionSharpness > 0.0) ||
            (add < MotionSharpness && MotionSharpness < 0.0))
        {
            add = MotionSharpness;
        }

        setSharpness += add;
        setSharpness = clamp(setSharpness, 0.0, 1.0);
    }

    float3 e = loadColor(pixel);

    // Skip sharpening if set value == 0
    if (setSharpness == 0.0)
    {
        if (Debug > 0 && DynamicSharpenEnabled > 0 && Sharpness > 0.0)
            e.g *= 1.0 + (12.0 * Sharpness);

        Dest[pixel] = storeColor(pixel,e);
        return;
    }

    // Clamp neighbor accesses at image borders
    int2 coordB = int2(pixel.x, max(pixel.y - 1, 0));
    int2 coordD = int2(max(pixel.x - 1, 0), pixel.y);
    int2 coordF = int2(min(pixel.x + 1, maxPixel.x), pixel.y);
    int2 coordH = int2(pixel.x, min(pixel.y + 1, maxPixel.y));

    float3 b = loadColor(coordB);
    float3 d = loadColor(coordD);
    float3 f = loadColor(coordF);
    float3 h = loadColor(coordH);

    float localScale = 1.0; // HDR input is perceptually encoded and bounded.

    float3 en = max(e / localScale, 0.0);
    float3 bn = max(b / localScale, 0.0);
    float3 dn = max(d / localScale, 0.0);
    float3 fn = max(f / localScale, 0.0);
    float3 hn = max(h / localScale, 0.0);

    // Min and max of normalized ring
    float3 minRGB = min(min(bn, dn), min(fn, hn));
    float3 maxRGB = max(max(bn, dn), max(fn, hn));    

    float2 peakC = float2(1.0, -4.0);

    // More numerically stable RCAS limiters
    float3 hitMin = minRGB / max(4.0 * maxRGB, 1e-5);
    float3 hitMax = (peakC.xxx - maxRGB) / min(4.0 * minRGB + peakC.yyy, -1e-5);

    float3 lobeRGB = max(-hitMin, hitMax);
    float lobe = max(-0.1875, min(max(lobeRGB.r, max(lobeRGB.g, lobeRGB.b)), 0.0)) * setSharpness;

    // OptiScaler contrast term, with its blend mapped over the full 0..2 range.
    if (Contrast > 0) {
        float3 amp = rsqrt(max(saturate(min(minRGB, 2-maxRGB) / max(maxRGB,1e-5)),1e-5));
        float factor = 1 / max(amp.g * (8-3*Contrast),1);
        lobe *= lerp(1.0,factor,Contrast*0.5);
    }

    float rcpL = rcp(4.0 * lobe + 1.0);
    float3 output = (((bn + dn + fn + hn) * lobe + en) * rcpL) * localScale;

    if (Debug > 0 && DynamicSharpenEnabled > 0)
    {
        if (Sharpness < setSharpness)
            output.r *= 1.0 + (12.0 * (setSharpness - Sharpness));
        else
            output.g *= 1.0 + (12.0 * (Sharpness - setSharpness));
    }

    Dest[pixel] = storeColor(pixel,output);
}
)RSF";
}
