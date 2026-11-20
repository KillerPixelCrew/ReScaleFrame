// SPDX-License-Identifier: GPL-3.0-only
// Shared bindings and color transport for the depth-aware passes. See THIRD_PARTY.md.
R"RSF(
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
    return pow(abs(x / (x + 4)),1.0/2.2);
}
float3 loadColor(int2 p) { return encodeColor(Source.Load(int3(p,0)).rgb); }
static const float3 kLumaCoeff = float3(0.2126, 0.7152, 0.0722);

float4 storeColor(int2 p, float3 c) {
    float4 original = Source.Load(int3(p,0));
    float3 center = encodeColor(original.rgb);
    if (ClampOutput == 0 && all(c == center)) return original;

    float3 lo = original.rgb, hi = original.rgb;
    if (EncodeHdr != 0) {
        if (Debug == 0) {
            // Retain the original local channel bounds through HDR decoding.
            [unroll] for (int y=-1;y<=1;++y) [unroll] for (int x=-1;x<=1;++x) {
                int2 q = clamp(p+int2(x,y),int2(0,0),int2(DisplayWidth,DisplayHeight)-1);
                float3 v = Source.Load(int3(q,0)).rgb;
                lo = min(lo,v);
                hi = max(hi,v);
            }
            if (ClampOutput == 0 && all(lo == hi)) return original;
            c = clamp(c,encodeColor(lo),encodeColor(hi));
        }
        float3 t = pow(clamp(c,0,0.999999),2.2);
        c = 4*t / max(1-t,1e-7) / exposureValue();
    }

    if (Debug == 0) {
        // The filter determines luminance. Apply its change to the source in
        // output space, where a single RGB gain preserves chromaticity.
        float luminance = dot(original.rgb,kLumaCoeff);
        float gain = luminance > 1e-6 ? max(dot(c,kLumaCoeff),0) / luminance : 1;
        if (EncodeHdr != 0) {
            [unroll] for (uint channel=0;channel<3;++channel) {
                if (original[channel] != 0) {
                    float a = lo[channel] / original[channel];
                    float b = hi[channel] / original[channel];
                    gain = clamp(gain,min(a,b),max(a,b));
                }
            }
        }
        if (ClampOutput != 0) {
            float peak = max(original.r,max(original.g,original.b));
            if (peak > 0) gain = min(gain,1/peak);
        }
        c = original.rgb * gain;
    }
    if (ClampOutput != 0) c = saturate(c);
    return float4(c,original.a);
}
)RSF"
