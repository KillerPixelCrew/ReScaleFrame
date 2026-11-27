
cbuffer Params : register(b0)
{
    float2 InverseTargetSize;
    float2 Padding;
};

struct VertexIn
{
    float2 position : POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

struct VertexOut
{
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

VertexOut vertex_main(VertexIn input)
{
    VertexOut output;

    // Physical pixels to clip space, using the target size the caller passed rather than a
    // projection matrix. y is negated because pixel coordinates grow downwards and clip space
    // upwards.
    output.position = float4(input.position.x * InverseTargetSize.x - 1.0,
                             1.0 - input.position.y * InverseTargetSize.y,
                             0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

Texture2D    Atlas        : register(t0);
SamplerState AtlasSampler : register(s0);

float4 pixel_main(VertexOut input) : SV_Target
{
    // Both the vertex colour and the atlas are premultiplied, so a component wise product is
    // premultiplied again and the blend state is the only remaining half of it.
    return input.color * Atlas.Sample(AtlasSampler, input.uv);
}
