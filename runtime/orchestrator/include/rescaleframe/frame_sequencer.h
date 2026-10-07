/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_FRAME_SEQUENCER_H
#define RSF_FRAME_SEQUENCER_H
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_frame_sequencer rsf_frame_sequencer;
/* Numeric snapshot only. Bit/timestamp order is input, simulation start/end, render submit
   start/end, Present start/end. Timestamps share the caller's monotonic clock (normally QPC). */
typedef struct rsf_sequence_state {
    uint32_t struct_size;
    uint64_t frame_id;
    uint32_t marker_mask;
    uint32_t failed;
    uint64_t timestamps[7];
} rsf_sequence_state;
/* A bounded CPU/input-to-present ledger. SDK calls happen at the real caller's boundary,
   outside the ledger's lock. Operations are internally synchronized. capacity is 1..64;
   frame IDs are nonzero and strictly increasing. finish() is required before slot reuse. */
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_create(uint32_t capacity, rsf_frame_sequencer** out);
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_begin(rsf_frame_sequencer*, uint64_t frame_id);
/* Accept each marker exactly once in order with nondecreasing, nonzero timestamps. An ordering
   error marks the frame failed, so subsequent markers refuse until finish discards it. */
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_marker(rsf_frame_sequencer*, uint64_t frame_id,
    rsf_latency_marker marker, uint64_t timestamp);
/* Copy an existing frame into caller storage initialized with struct_size. */
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_get(rsf_frame_sequencer*, uint64_t frame_id,
    rsf_sequence_state* state);
/* Always clear a matching slot. Returns OK only if all seven markers completed without error;
   NOT_READY can therefore mean the incomplete frame was successfully discarded. */
RSF_RUNTIME_API rsf_backend_result rsf_frame_sequencer_finish(rsf_frame_sequencer*, uint64_t frame_id);
/* Caller stops all users first. Null is allowed; pending CPU entries are discarded. */
RSF_RUNTIME_API void rsf_frame_sequencer_destroy(rsf_frame_sequencer*);
#ifdef __cplusplus
}
#endif
#endif
