
Texture2D<float4> Source : register(t0);
Texture2D<float> Exposure : register(t1);
Texture2D<float4> Scene : register(t2);
SamplerState LinearClamp : register(s0);
RWTexture2D<float4> Result : register(u0);
cbuffer Params : register(b0) {
    uint2 Size;
    uint HasExposure;
    uint HasScene;
    float2 SceneView;
    float2 SceneTexture;
    float2 Jitter;
    float2 Padding;
};
static const float Knee = 4;
float exposure() {
    float value = HasExposure != 0 ? Exposure.Load(int3(0,0,0)) : 1;
    return isfinite(value) && value > 0 ? value : 1;
}
[numthreads(8,8,1)] void encode(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float4 colour = Source.Load(int3(id.xy,0));
    float3 scaled = max(isfinite(colour.rgb) ? colour.rgb : 0, 0) * exposure();
    Result[id.xy] = float4(pow(scaled / (scaled + Knee), 1 / 2.2), colour.a);
}
[numthreads(8,8,1)] void decode(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float4 colour = Source.Load(int3(id.xy,0));
    // Below 1 by one half-float step of the encoded value, the largest finite inverse. The
    // reconstruction saturates its display-range output at 1, so nothing above this survives it.
    float3 y = clamp(colour.rgb, 0, 0.99951);
    float3 t = pow(y, 2.2);
    float3 linearColour = Knee * t / (1 - t) / exposure();
    if (HasScene != 0) {
        // In the saturated band the output says only "at least this bright". Take the current
        // frame's own HDR value there, sampled where this output pixel lies in the jittered render
        // grid, never below the value at the start of the band.
        float2 viewUV = (float2(id.xy) + 0.5) / float2(Size) + Jitter / SceneView;
        float3 scene = max(Scene.SampleLevel(LinearClamp, viewUV * SceneView / SceneTexture, 0).rgb, 0);
        const float bandStart = 0.998;
        float tStart = pow(bandStart, 2.2);
        float floorValue = Knee * tStart / (1 - tStart) / exposure();
        float weight = saturate((max(y.r, max(y.g, y.b)) - bandStart) / 0.0015);
        linearColour = lerp(linearColour, max(scene, min(linearColour, floorValue)), weight);
    }
    Result[id.xy] = float4(linearColour, colour.a);
}