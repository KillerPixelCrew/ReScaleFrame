/* SPDX-License-Identifier: GPL-3.0-only */
/* Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
   work (see docs/representation-plan.md). Do not delete as dead code. */
#ifndef RSF_FRAME_SEQUENCER_H
#define RSF_FRAME_SEQUENCER_H
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_frame_sequencer rsf_frame_sequencer;
typedef struct rsf_sequence_state {
    uint32_t struct_size;
    uint64_t frame_id;
    uint32_t marker_mask;
    uint32_t failed;
    uint64_t timestamps[7];
} rsf_sequence_state;
/* A bounded CPU/input-to-present ledger. SDK calls happen at the real caller's boundary,
   outside the ledger's lock. finish() is required before the same slot can be reused. */
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_create(uint32_t capacity, rsf_frame_sequencer** out);
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_begin(rsf_frame_sequencer*, uint64_t frame_id);
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_marker(rsf_frame_sequencer*, uint64_t frame_id,
    rsf_latency_marker marker, uint64_t timestamp);
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_get(rsf_frame_sequencer*, uint64_t frame_id,
    rsf_sequence_state* state);
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_finish(rsf_frame_sequencer*, uint64_t frame_id);
RSF_RUNTIME_API void rsf_frame_sequencer_destroy(rsf_frame_sequencer*);
#ifdef __cplusplus
}
#endif
#endif
