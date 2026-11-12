// First-party family-specific previous-state ABI. VS slot b13 is reserved.
#ifndef RSF_ANOMALY_FOLIAGE_H
#define RSF_ANOMALY_FOLIAGE_H
cbuffer RSFFoliagePrevious : register(b13)
{
    row_major float4x4 rsf_previous_clip_from_world;
    row_major float3x4 rsf_previous_tree_xform;
    float4 rsf_previous_consts;
    float4 rsf_previous_wave;
    float4 rsf_previous_wind;
    // x: complete draw history, y: current detail batch count, z: ABI version 1.
    uint4 rsf_foliage_history;
    // Native remaps stable item histories to CURRENT groups of four rows.
    // Rows 0..2 already include previous effective scale/fade, row 3 is colour.
    float4 rsf_previous_rows[61 * 4];
    // x: complete identity-matched history, y: effective scale, z: fade, w: reserved.
    float4 rsf_previous_item_state[61];
};
bool rsf_foliage_draw_valid()
{
    return rsf_foliage_history.x == 1 && rsf_foliage_history.z == 1 &&
        rsf_foliage_history.w == 0 &&
        all(isfinite(rsf_previous_clip_from_world[0])) &&
        all(isfinite(rsf_previous_clip_from_world[1])) &&
        all(isfinite(rsf_previous_clip_from_world[2])) &&
        all(isfinite(rsf_previous_clip_from_world[3])) &&
        all(isfinite(rsf_previous_consts)) && all(isfinite(rsf_previous_wave)) &&
        all(isfinite(rsf_previous_wind));
}
bool rsf_tree_history_valid()
{
    return rsf_foliage_draw_valid() &&
        all(isfinite(rsf_previous_tree_xform[0])) &&
        all(isfinite(rsf_previous_tree_xform[1])) &&
        all(isfinite(rsf_previous_tree_xform[2]));
}
bool rsf_finite_row(float4 value)
{
    // FXC 10.0.26100 crashes on isfinite(dynamic-cbuffer-array-load) here.
    // IEEE exponent-all-ones rejects both infinity and NaN without that lowering.
    return all((asuint(value) & 0x7f800000u) != 0x7f800000u);
}
bool rsf_detail_history_valid(int row)
{
    // Branch before ANY dynamic prior-buffer reads. HLSL && is not short-circuit.
    if (!rsf_foliage_draw_valid() || row < 0 || row > 240 || (uint(row) & 3u) != 0 ||
        rsf_foliage_history.y > 61 || (uint(row) >> 2u) >= rsf_foliage_history.y)
        return false;
    float4 state = rsf_previous_item_state[uint(row) >> 2u];
    return all(isfinite(state)) && state.x == 1 && state.y > 0 &&
        state.z > 0 && state.z <= 1 && state.w == 0 &&
        rsf_finite_row(rsf_previous_rows[row]) &&
        rsf_finite_row(rsf_previous_rows[row + 1]) &&
        rsf_finite_row(rsf_previous_rows[row + 2]) &&
        rsf_finite_row(rsf_previous_rows[row + 3]);
}
#endif
