// First-party per-draw temporal ABI 1. Bind to the VS only.
#ifndef RSF_CORE_VERTEX_H
#define RSF_CORE_VERTEX_H
cbuffer RSFCorePrevious : register(b13)
{
    // Row-major memory; mul(matrix, column-vector). Same convention as m_WVP.
    row_major float4x4 rsf_previous_wvp : packoffset(c0);
    float4 rsf_previous_bones[234] : packoffset(c4);
    // prior draw valid, palette valid, SKIN_NONE known rigid, ABI (=1).
    float4 rsf_history : packoffset(c238);
};
float rsf_draw_valid()
{
    return (rsf_history.x == 1 && rsf_history.w == 1) ? 1.0 : 0.0;
}
#endif
