// SPDX-License-Identifier: MIT
Texture2D<float> depth_in : register(t0);
Texture2D<float4> motion_in : register(t1);
RWTexture2D<float> depth_out : register(u0);
RWTexture2D<float2> motion_out : register(u1);
cbuffer Constants : register(b0) {
    row_major float4x4 clip_to_previous;
    float2 size;
    float has_motion;
    float first_person_limit;
    float validity_threshold;
    float3 padding;
};
[numthreads(8, 8, 1)]
void rsf_fg_inputs_cs(uint3 id : SV_DispatchThreadID) {
    if (id.x >= (uint)size.x || id.y >= (uint)size.y) return;
    const float depth = depth_in[id.xy];
    depth_out[id.xy] = depth;
    const float2 uv = (float2(id.xy) + 0.5) / size;
    const float4 stored = has_motion > 0.5 ? motion_in[id.xy] : float4(0, 0, 0, 0);
    float2 motion;
    if (stored.z > validity_threshold) {
        motion = -stored.xy * size;                  // current-minus-previous UV -> previous-minus-current pixels
    } else if (depth < first_person_limit) {
        motion = float2(0, 0);                       // first-person geometry has its own projection
    } else {
        const float4 previous = mul(float4(uv.x * 2 - 1, 1 - uv.y * 2, depth, 1), clip_to_previous);
        const float2 ndc = previous.xy / previous.w;
        motion = (float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) - uv) * size;
    }
    motion_out[id.xy] = motion;
}
