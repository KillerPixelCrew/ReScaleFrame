/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_CPU_H
#define RSF_NATIVE_CPU_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_native_cpu_frame {
    uint32_t struct_size;
    uint32_t stage_mask;
    uint64_t session_id;
    uint64_t source_frame_id;
    uint64_t qpc_frequency;
    uint64_t timestamps_qpc[5];
    uint32_t ended;
    uint32_t failed;
    uint32_t input_mask;
    uint32_t input_events[3];
    uint64_t input_qpc[3];
    uint64_t pacing_qpc, input_ready_qpc;
    uint32_t flags, marker_mask, pings;

} rsf_native_cpu_frame;
typedef struct rsf_native_cpu_counters {
    uint32_t struct_size;
    uint64_t frames, input_events[3], pings, substitutes, duplicates, order_violations, expired_messages;
} rsf_native_cpu_counters;
RSF_RUNTIME_API int rsf_native_cpu_counters_get(rsf_native_cpu_counters*);
/* Core endpoints register the SDK-owned Windows ping identifier, never an adapter. */
RSF_RUNTIME_API void rsf_native_cpu_ping_message(uint32_t message);
/* SDK marker 0..5: 1 accepted, 0 duplicate, -1 missing/out of order. */
RSF_RUNTIME_API int rsf_native_cpu_marker(uint64_t source, uint32_t marker);
RSF_RUNTIME_API void rsf_native_cpu_paced(uint64_t source, uint64_t began, uint64_t ready);
/* Copies CPU ownership/ordering only, with no GPU or presentation authorization. A bounded
   cache may reuse ended records; absent or overwritten IDs refuse lookup. Live records never
   evict. The optional vendor sink runs at the real CPU boundary, outside the cache lock. */
RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event);
/* Runtime hosts deliver accepted and normalized facts to their backend endpoint. No adapter supplies this sink. */
RSF_RUNTIME_API int rsf_native_cpu_dispatch(const rsf_game_cpu_event* event, rsf_game_cpu_event_fn sink, void* user);
RSF_RUNTIME_API int rsf_native_cpu_read(uint64_t session, uint64_t source, rsf_native_cpu_frame* frame);
/* Set/change the sink only while game CPU producers are quiescent. User state must outlive
   callbacks. The sink may sleep/mark the vendor; it must not use the graphics immediate context. */
/* Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
   work (see docs/representation-plan.md). Do not delete as dead code. */
RSF_RUNTIME_API void rsf_native_cpu_set_sink(rsf_game_cpu_event_fn sink, void* user);
/* After the plugin has stopped CPU producers. */
RSF_RUNTIME_API void rsf_native_cpu_release(void);
#ifdef __cplusplus
}
#endif
#endif
