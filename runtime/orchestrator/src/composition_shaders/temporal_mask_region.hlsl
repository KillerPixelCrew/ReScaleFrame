
Texture2D<float4> Source : register(t0);
Texture2D<float> Reactive : register(t1);
Texture2D<float> Bias : register(t2);
RWTexture2D<float> OutReactive : register(u0);
RWTexture2D<float> OutBias : register(u1);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; float4 Weights; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) {
        float hint = dot(Source.Load(int3(Origin + id.xy, 0)), Weights);
        hint = isfinite(hint) ? saturate(hint) : 1;
        OutReactive[id.xy] = max(hint, Reactive.Load(int3(id.xy, 0)));
        OutBias[id.xy] = max(hint, Bias.Load(int3(id.xy, 0)));
    }
}