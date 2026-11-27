Texture2D<float4> Source : register(t0);
cbuffer Params : register(b0) {
    float4 SourceRect;
    float4 OutputRect;
    float4 ValidTexels;
    float2 Jitter;
    float2 Padding;
};
float4 vertex_main(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
float4 pixel_main(float4 position : SV_Position) : SV_Target {
    float2 uv = (position.xy - OutputRect.xy) / (OutputRect.zw - OutputRect.xy);
    float2 p = floor(SourceRect.xy + uv * (SourceRect.zw - SourceRect.xy) - Jitter);
    return Source.Load(int3(int2(clamp(p, ValidTexels.xy, ValidTexels.zw)), 0));
}
