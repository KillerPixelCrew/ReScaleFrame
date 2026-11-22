// SPDX-License-Identifier: MIT
// HdrConstants offsets measured in F9 captures of composite_tile (PS CRC32C 83524e47):
// tanHalfFov at 448, depthToLinFadeDistParams at 464, maxFadeDistanceKm at 580,
// maxCloudDistanceKm at 636. The composite converts device depth to normalized radial distance
// with sqrt(1+|ndc*tanHalfFov|^2) * x/(d*y+z) (+ d*w); with z = w = 0, as captured, this inverts
// to d = x / (y * planar). The near/far texture's z channel is km along the ray to the first 25%
// opacity, 150 (maxFade) when the ray never reaches it and 100 (maxCloud) when it misses the layer.
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
}