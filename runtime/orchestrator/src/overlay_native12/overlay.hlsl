
cbuffer View : register(b0) { float2 inverseSize; };
struct V { float2 p:POSITION; float2 uv:TEXCOORD0; float4 c:COLOR0; };
struct P { float4 p:SV_Position; float2 uv:TEXCOORD0; float4 c:COLOR0; };
P vs(V v) { P p; p.p=float4(v.p.x*inverseSize.x-1,1-v.p.y*inverseSize.y,0,1);p.uv=v.uv;p.c=v.c;return p; }
Texture2D atlas:register(t0); SamplerState linearSampler:register(s0);
float4 ps(P p):SV_Target { return p.c*atlas.Sample(linearSampler,p.uv); }
