
Texture2D<float2> Motion : register(t0);
Texture2D<float> Depth : register(t1);
Texture2D<float> DepthLayer : register(t2);
RWTexture2D<float2> Resolved : register(u0);
RWTexture2D<float> ResolvedDepth : register(u1);
cbuffer Params : register(b0) {
    row_major float4x4 ClipToPrevious;
    float2 DecodedToPixels;
    float Sentinel;
    uint HasSentinel;
    uint2 Size;
    uint HasLayer;
    uint Padding;
    float2 JitterPixels;
    float2 Padding2;
};
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float depth = Depth[id.xy];
    float2 decoded = Motion[id.xy];
    float2 motion = decoded * DecodedToPixels;
    if (HasSentinel != 0 && all(decoded == Sentinel)) {
        // Depth belongs to a raster sample shifted by the current projection. Both matrices and
        // written vectors exclude jitter, so reconstruct at the corresponding unjittered position.
        float2 ndc = (float2(id.xy) + 0.5 - JitterPixels) / float2(Size) * float2(2,-2) + float2(-1,1);
        // Reversed Z: the larger value is the nearer surface, such as a cloud in front of sky.
        float reprojected = HasLayer != 0 ? max(depth, DepthLayer[id.xy]) : depth;
        float4 previous = mul(float4(ndc, reprojected, 1), ClipToPrevious);
        motion = abs(previous.w) > 1e-8 ? (previous.xy / previous.w - ndc) * float2(Size) * float2(0.5,-0.5) : 0;
    }
    Resolved[id.xy] = all(isfinite(motion)) ? motion : 0;
    ResolvedDepth[id.xy] = depth;
}