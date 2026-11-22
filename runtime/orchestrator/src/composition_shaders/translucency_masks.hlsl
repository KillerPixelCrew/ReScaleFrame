
Texture2D<float4> Scene : register(t0);
Texture2D<float4> Opaque : register(t1);
Texture2D<float4> Layer : register(t2);
SamplerState Linear : register(s0);
RWTexture2D<float> Reactive : register(u0);
RWTexture2D<float> Coverage : register(u1);
RWTexture2D<float> Bias : register(u2);
cbuffer Params : register(b0) {
    int2 Origin;
    uint2 Size;
    float2 SceneInvSize;
    uint Mode;
    float CoverageScale;
};
float3 tonemap(float3 c) { c = max(c, 0); return c / (1 + max(c.r, max(c.g, c.b))); }
float change(float3 a, float3 b) { float3 d = abs(tonemap(a) - tonemap(b)); return max(d.r, max(d.g, d.b)); }
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    int2 pixel = Origin + int2(id.xy);
    float3 scene = Scene.Load(int3(pixel, 0)).rgb;
    float reactive, coverage;
    if (Mode == 0) {
        reactive = change(scene, Opaque.Load(int3(id.xy, 0)).rgb);
        coverage = saturate(reactive * CoverageScale);
    } else {
        float4 layer = Layer.SampleLevel(Linear, (float2(pixel) + 0.5) * SceneInvSize, 0);
        float opacity = saturate(1 - layer.a);
        float visible = change(scene * saturate(layer.a) + max(layer.rgb, 0), scene);
        reactive = max(Reactive[id.xy], max(visible, opacity));
        coverage = max(Coverage[id.xy], max(opacity, saturate(visible * CoverageScale)));
    }
    reactive = isfinite(reactive) ? reactive : 1;
    coverage = isfinite(coverage) ? coverage : 1;
    Reactive[id.xy] = min(reactive, 0.9);
    Coverage[id.xy] = coverage;
    Bias[id.xy] = coverage >= 0.5 ? 1 : 0;
}