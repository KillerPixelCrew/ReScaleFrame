// SPDX-License-Identifier: GPL-3.0-only
// First-party declaration prototype. No geometry or pixel output is modified.
// Deliberately no fixed register: reflected binding must be established per
// compiled variant before any runtime upload. Unused declarations can disappear.
#ifndef RSF_ANOMALY_TEMPORAL_DECLARATIONS_V1
#define RSF_ANOMALY_TEMPORAL_DECLARATIONS_V1 1

cbuffer RSFAnomalyTemporalPrototype
{
    row_major float4x4 rsf_previous_object_to_clip;
    row_major float4x4 rsf_previous_world_to_clip;
    float4 rsf_current_previous_jitter_uv;
    uint4 rsf_history_flags;
};

#endif
