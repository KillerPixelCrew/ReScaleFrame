
Texture2D<float4> Source : register(t0);
SamplerState      Sampler : register(s0);

cbuffer Params : register(b0)
{
    uint  Mode;
    float Exposure;
    uint2 Padding;
};

struct Varying
{
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

Varying vertex_main(uint id : SV_VertexID)
{
    Varying output;
    output.uv = float2((id << 1) & 2, id & 2);
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

float4 pixel_main(Varying input) : SV_Target
{
    float4 source = Source.Sample(Sampler, input.uv);
    if (Mode == 1)
    {
        // Reinhard and a gamma. Enough to see a linear image; not a grade, and not what the game
        // does. Scene colour here runs well past one, so without this the picture is nearly black.
        float3 colour = source.rgb * Exposure;
        colour = colour / (colour + 1.0);
        return float4(pow(saturate(colour), 1.0 / 2.2), 1.0);
    }
    if (Mode == 2 || Mode == 4 || Mode == 5)
    {
        // Premultiplied. The colour is emitted as it is and the blend supplies
        // `ui.rgb + (1 - ui.a) * dst`, so nothing is divided by alpha here: a layer written by
        // over blends is already premultiplied, and dividing would undo exactly the thing that
        // makes it composite correctly where coverage is partial.
        //
        // The encode is the destination's, not the source's. AC7 stores its interface as linear
        // values in a plain UNORM target and applies the display transform in a later pass; the
        // back buffer this composites onto already holds transformed colour. Blending one into the
        // other without the transform is what makes the interface arrive dark.
        //
        // Applied to the premultiplied colour rather than to a divided one. That is not exact for
        // partial coverage, and it is what a compositor working in display space does; the
        // alternative costs a divide and a multiply per pixel to be differently approximate.
        if (Mode == 4)
        {
            float3 low = source.rgb * 12.92;
            float3 high = 1.055 * pow(max(source.rgb, 0.0), 1.0 / 2.4) - 0.055;
            return float4(source.rgb <= 0.0031308 ? low : high, source.a);
        }
        if (Mode == 5)
        {
            return float4(pow(max(source.rgb, 0.0), 1.0 / 2.2), source.a);
        }
        return source;
    }
    if (Mode == 3)
    {
        return float4(source.aaa, 1.0);
    }
    if (Mode == 6) return source;
    return float4(source.rgb, 1.0);
}
