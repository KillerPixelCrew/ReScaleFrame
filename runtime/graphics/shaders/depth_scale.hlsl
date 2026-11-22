
Texture2D<float> Depth : register(t0);
Texture2D<uint2> Stencil : register(t1);
cbuffer Params : register(b0) {
    float4 SourceRect;
    float4 OutputRect;
    float2 Jitter;
    uint Bit;
    uint Padding;
};
float4 vertex_main(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
int2 source_position(float4 position) {
    float2 uv = (position.xy - OutputRect.xy) / (OutputRect.zw - OutputRect.xy);
    float2 p = floor(SourceRect.xy + uv * (SourceRect.zw - SourceRect.xy) - Jitter);
    return int2(clamp(p, SourceRect.xy, SourceRect.zw - 1));
}
float depth_main(float4 position : SV_Position) : SV_Depth {
    return Depth.Load(int3(source_position(position), 0));
}
void stencil_main(float4 position : SV_Position) {
    if ((Stencil.Load(int3(source_position(position), 0)).y & Bit) == 0) discard;
}
