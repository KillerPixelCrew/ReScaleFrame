// First-party motion writer. Native shading and coverage execute first.
#ifndef RSF_CORE_PIXEL_H
#define RSF_CORE_PIXEL_H
cbuffer RSFCorePixel : register(b13)
{
    // Current viewport origin.xy and reciprocal render viewport extent.zw.
    float4 rsf_viewport : packoffset(c0);
    // Current jitter UV.xy and previous jitter UV.zw, both positive Y down.
    float4 rsf_jitter : packoffset(c1);
    // x = ABI 1. Remaining lanes must be zero.
    float4 rsf_pixel_control : packoffset(c2);
};
struct RSFMotionOutput
{
    float4 native0 : SV_Target0;
    float4 native1 : SV_Target1;
    float4 motion : SV_Target2;
#ifdef EXTEND_F_DEFFER
    uint native_mask : SV_Coverage;
#endif
};
float4 rsf_encode_motion(float4 pixel, float4 previous_clip, float valid)
{
    // Previous clip is produced by the actual previous (possibly jittered) WVP.
    // Default perspective interpolation is essential. Do NOT use noperspective.
    if (valid != 1 || rsf_pixel_control.x != 1 ||
        !all(isfinite(previous_clip)) || previous_clip.w <= 0.000001 ||
        !all(isfinite(rsf_viewport)) || any(rsf_viewport.zw <= 0) ||
        !all(isfinite(rsf_jitter)))
        return float4(0, 0, 0, 0);
    float2 current_uv = (pixel.xy - rsf_viewport.xy) * rsf_viewport.zw - rsf_jitter.xy;
    float2 previous_uv = previous_clip.xy / previous_clip.w * float2(0.5, -0.5) + 0.5 - rsf_jitter.zw;
    float2 velocity = current_uv - previous_uv;
    // Reject half-float overflow instead of manufacturing a valid zero vector.
    if (!all(isfinite(velocity)) || any(abs(velocity) > 65504))
        return float4(0, 0, 0, 0);
    return float4(velocity, 1, 0);
}
#endif
