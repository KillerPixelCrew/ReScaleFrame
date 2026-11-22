
Texture2D<float4>   Source : register(t0);
RWTexture2D<float2> Target : register(u0);

cbuffer Params : register(b0)
{
    float2 Scale;
    float2 Bias;
    float2 OutputScale;
    float  InvalidValue;
    uint   ZeroMeansUnwritten;
    uint2  Size;
    uint   ValidityEnabled;
    uint   ValidityChannel;
    float  ValidityThreshold;
    uint3  Padding;
};

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Size.x || id.y >= Size.y)
    {
        return;
    }

    float4 raw = Source[id.xy];
    float2 stored = raw.xy;

    // The sentinel test runs before the decode and on the stored value, which is the only place it
    // is unambiguous. After the decode, zero is a value real motion can take. An explicit validity
    // channel replaces the zero test: validity then does not depend on the motion value at all.
    bool unwritten;
    if (ValidityEnabled != 0)
    {
        unwritten = !(raw[ValidityChannel & 3u] > ValidityThreshold);
    }
    else
    {
        unwritten = ZeroMeansUnwritten != 0 && stored.x == 0.0 && stored.y == 0.0;
    }
    if (unwritten)
    {
        Target[id.xy] = float2(InvalidValue, InvalidValue);
        return;
    }

    Target[id.xy] = (stored - Bias) * Scale * OutputScale;
}
