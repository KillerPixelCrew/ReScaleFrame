
Texture2D<float4> Scene : register(t0);
Texture2D<float4> Reconstructed : register(t1);
Texture2D<float4> Residual : register(t2);
Texture2D<float> Depth : register(t3);
RWTexture2D<float4> Result : register(u0);
SamplerState LinearClamp : register(s0);
cbuffer Params : register(b0) { uint2 RenderSize; uint2 OutputSize; float2 Jitter; uint DepthInverted; uint Padding; };
float sourceDepth(int2 p) {
    float value = Depth.Load(int3(clamp(p,int2(0,0),int2(RenderSize)-1),0));
    return DepthInverted != 0 ? value : 1-value;
}
float surfaceConfidence(float lower, float upper) {
    return 1-smoothstep(0.01,0.05,(upper-lower)/max(upper,1e-7));
}
[numthreads(8,8,1)] void project(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= RenderSize)) return;
    float2 scale = float2(OutputSize) / float2(RenderSize);
    float2 a = (float2(id.xy) - Jitter) * scale;
    float2 b = a + scale;
    int2 first = int2(floor(a)), last = int2(ceil(b));
    float3 sum = 0; float total = 0;
    [loop] for (int y = first.y; y < last.y; ++y) {
        [loop] for (int x = first.x; x < last.x; ++x) {
            float2 area = max(0, min(b,float2(x+1,y+1)) - max(a,float2(x,y)));
            float weight = area.x * area.y;
            sum += Reconstructed.Load(int3(clamp(int2(x,y),int2(0,0),int2(OutputSize)-1),0)).rgb * weight;
            total += weight;
        }
    }
    float3 expected = max(0,Scene.Load(int3(id.xy,0)).rgb);
    float lower = sourceDepth(int2(id.xy)), upper = lower;
    [unroll] for (int y = -1; y <= 1; ++y) {
        [unroll] for (int x = -1; x <= 1; ++x) {
            float neighbour = sourceDepth(int2(id.xy)+int2(x,y));
            lower = min(lower,neighbour); upper = max(upper,neighbour);
        }
    }
    float3 average = sum / max(total,1e-8);
    Result[id.xy] = float4((expected - average)*surfaceConfidence(lower,upper),0);
}
[numthreads(8,8,1)] void apply(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= OutputSize)) return;
    float2 uv = (float2(id.xy)+0.5) / float2(OutputSize) + Jitter / float2(RenderSize);
    float4 colour = Reconstructed.Load(int3(id.xy,0));
    float3 correction = Residual.SampleLevel(LinearClamp,uv,0).rgb;
    int2 first = int2(floor(uv * float2(RenderSize) - 0.5));
    float3 lower = colour.rgb, upper = colour.rgb;
    float depthLower = 3.402823e38, depthUpper = 0;
    [unroll] for (int y = 0; y < 2; ++y) {
        [unroll] for (int x = 0; x < 2; ++x) {
            float3 source = max(0,Scene.Load(int3(clamp(first+int2(x,y),int2(0,0),int2(RenderSize)-1),0)).rgb);
            lower = min(lower,source); upper = max(upper,source);
            float z = sourceDepth(first+int2(x,y));
            depthLower = min(depthLower,z); depthUpper = max(depthUpper,z);
        }
    }
    correction *= surfaceConfidence(depthLower,depthUpper);
    // Only move toward the source's colour range. Existing reconstructed detail remains valid
    // when the residual is zero; correction cannot introduce brighter peaks or dark ringing.
    Result[id.xy] = float4(clamp(colour.rgb+correction,lower,upper),colour.a);
}