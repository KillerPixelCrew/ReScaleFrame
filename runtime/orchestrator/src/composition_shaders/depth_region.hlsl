
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
}