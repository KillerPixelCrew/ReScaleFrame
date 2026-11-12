// First-party per-draw tessellation history, ABI 1. Bind to the DS only (b13); the motion VS keeps RSFCorePrevious.
// The motion VS hands the previous OBJECT-space position and normal of every control point to the hull stage; the
// domain stage takes them to the previous view space here, rebuilds the previous PN patch there (the same space the
// native patch is built in) and projects the previous surface point with the previous projection.
#ifndef RSF_CORE_TESS_H
#define RSF_CORE_TESS_H
cbuffer RSFTessPrevious : register(b13)
{
    // Row-major memory; mul(matrix, column-vector), like rsf_previous_wvp. Prior V * W (bone-composed for SKIN_0).
    row_major float4x4 rsf_previous_wv : packoffset(c0);
    // Prior (possibly jittered) projection: rsf_previous_projection * rsf_previous_wv == rsf_previous_wvp.
    row_major float4x4 rsf_previous_projection : packoffset(c4);
    // x = prior draw valid, w = ABI (=1). Remaining lanes must be zero.
    float4 rsf_tess_history : packoffset(c8);
};
float3 rsf_tess_view_position(float4 previous_object)
{
    return mul(rsf_previous_wv, previous_object).xyz;
}
float3 rsf_tess_view_normal(float3 previous_object)
{
    return mul((float3x3)rsf_previous_wv, previous_object);
}
float rsf_tess_valid(float a, float b, float c)
{
    return (a == 1 && b == 1 && c == 1 && rsf_tess_history.x == 1 && rsf_tess_history.w == 1) ? 1.0 : 0.0;
}
// Rigid frame of a control triangle (rows: first edge, in-plane perpendicular, face normal).
float3x3 rsf_tess_frame(float3 p0, float3 p1, float3 p2)
{
    float3 e = normalize(p1 - p0);
    float3 n = normalize(cross(p1 - p0, p2 - p0));
    return float3x3(e, cross(n, e), n);
}
// A vector expressed in the current triangle's frame, re-expressed in the previous triangle's frame.
float3 rsf_tess_carry(float3x3 current, float3x3 previous, float3 v)
{
    return mul(mul(current, v), previous);
}
#ifdef TESS_H_INCLUDED
// The patch the hull stage delivered for THIS frame need not be the textbook PN construction on this frame's control
// points (the engine's d3dcompiler_43 build of the TESS_HM hull shader writes some patch components from several
// fork-phase instances). The previous patch is the textbook construction on the previous control points plus this
// frame's delivered-minus-textbook difference, carried with the triangle's rigid frame: identical to the delivered patch
// when nothing moved, exact under rigid motion, and the plain textbook patch when the hull stage delivers it.
void rsf_tess_carry_patch(float3 current[3], float3 previous[3], PNPatch delivered, PNPatch reference, inout PNPatch patch)
{
    float3x3 c = rsf_tess_frame(current[0], current[1], current[2]);
    float3x3 p = rsf_tess_frame(previous[0], previous[1], previous[2]);
    patch.f3B210 += rsf_tess_carry(c, p, delivered.f3B210 - reference.f3B210);
    patch.f3B120 += rsf_tess_carry(c, p, delivered.f3B120 - reference.f3B120);
    patch.f3B021 += rsf_tess_carry(c, p, delivered.f3B021 - reference.f3B021);
    patch.f3B012 += rsf_tess_carry(c, p, delivered.f3B012 - reference.f3B012);
    patch.f3B102 += rsf_tess_carry(c, p, delivered.f3B102 - reference.f3B102);
    patch.f3B201 += rsf_tess_carry(c, p, delivered.f3B201 - reference.f3B201);
    patch.f3B111 += rsf_tess_carry(c, p, delivered.f3B111 - reference.f3B111);
    patch.f3N110 += rsf_tess_carry(c, p, delivered.f3N110 - reference.f3N110);
    patch.f3N011 += rsf_tess_carry(c, p, delivered.f3N011 - reference.f3N011);
    patch.f3N101 += rsf_tess_carry(c, p, delivered.f3N101 - reference.f3N101);
}
#endif
#endif
