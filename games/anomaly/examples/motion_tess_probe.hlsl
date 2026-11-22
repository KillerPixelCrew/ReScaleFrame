// SPDX-License-Identifier: MIT
struct VSOut { float4 pos : POSITION0; float4 tc0 : TEXCOORD0; float4 rsf_previous_clip : TEXCOORD12; nointerpolation float rsf_valid : TEXCOORD13; };
VSOut vs_main(uint id : SV_VertexID) {
    float2 p = float2((id << 1) & 2, id & 2);
    VSOut o;
    o.pos = float4(p * float2(2, -2) + float2(-1, 1), 0.5, 1);
    o.tc0 = float4(0.25, 0.5, 0.75, 1);
    o.rsf_previous_clip = float4(0.125, -0.25, 0.5, 1);
    o.rsf_valid = 1;
    return o;
}
struct DirectOut { float4 tc0 : TEXCOORD0; float4 rsf_previous_clip : TEXCOORD12; nointerpolation float rsf_valid : TEXCOORD13; float4 hpos : SV_Position; };
DirectOut vs_direct(uint id : SV_VertexID) {
    const VSOut v = vs_main(id);
    DirectOut o;
    o.tc0 = v.tc0; o.rsf_previous_clip = v.rsf_previous_clip; o.rsf_valid = v.rsf_valid; o.hpos = v.pos;
    return o;
}
struct HSIn { float4 pos : POSITION0; float4 tc0 : TEXCOORD0; };          // the stock control point: no RSF fields
struct HSConstants { float edges[3] : SV_TessFactor; float inside : SV_InsideTessFactor; };
HSConstants hs_constants(InputPatch<HSIn, 3> ip) { HSConstants c; c.edges[0] = c.edges[1] = c.edges[2] = 2; c.inside = 2; return c; }
[domain("tri")] [partitioning("integer")] [outputtopology("triangle_cw")] [outputcontrolpoints(3)] [patchconstantfunc("hs_constants")]
HSIn hs_main(InputPatch<HSIn, 3> ip, uint i : SV_OutputControlPointID) { return ip[i]; }
struct DSOut { float4 tc0 : TEXCOORD0; float4 hpos : SV_Position; };      // the stock v2p_bumped-like output
[domain("tri")]
DSOut ds_main(HSConstants c, float3 uvw : SV_DomainLocation, const OutputPatch<HSIn, 3> bp) {
    DSOut o;
    o.hpos = bp[0].pos * uvw.x + bp[1].pos * uvw.y + bp[2].pos * uvw.z;
    o.tc0 = bp[0].tc0 * uvw.x + bp[1].tc0 * uvw.y + bp[2].tc0 * uvw.z;
    return o;
}
struct PSIn { float4 tc0 : TEXCOORD0; float4 rsf_previous_clip : TEXCOORD12; nointerpolation float rsf_valid : TEXCOORD13; float4 hpos : SV_Position; };
float4 ps_main(PSIn i) : SV_Target { return float4(i.rsf_previous_clip.xy, i.rsf_valid, i.tc0.x); }
