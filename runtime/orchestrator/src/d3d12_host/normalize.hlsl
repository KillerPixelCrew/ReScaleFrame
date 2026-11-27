
Texture2D<float4> scene : register(t0);
Texture2D<float> depth : register(t1);
Texture2D<float2> motion : register(t2);
Texture2D<float> exposureInput : register(t3);
RWTexture2D<float4> sceneOut : register(u0);
RWTexture2D<float> depthOut : register(u1);
RWTexture2D<float2> motionOut : register(u2);
RWTexture2D<float> exposureOut : register(u3);
cbuffer Constants : register(b0) { uint2 extent; float2 toPixels; uint inputsOnly; uint externalExposure; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= extent)) return;
    if (!inputsOnly) sceneOut[id.xy] = scene.Load(int3(id.xy,0));
    depthOut[id.xy] = depth.Load(int3(id.xy,0));
    motionOut[id.xy] = motion.Load(int3(id.xy,0)) * toPixels;
    if (externalExposure && all(id.xy == 0)) exposureOut[uint2(0,0)] = exposureInput.Load(int3(0,0,0));
}