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
    uint64_t frames, input_events[3], pings, substitutes, duplicates, order_violations, expired_messages, unframed_inputs, derived_frame_ends;
} rsf_native_cpu_counters;
/* Shared latency status, independent of the selected rendering provider. Version 1. */
typedef struct rsf_native_latency_status {
    uint32_t struct_size, version, owner, available, effective_mode, pcl_available, report_available;
    uint64_t paced_frames, marker_calls, derived_boundaries;
    rsf_native_cpu_counters events;
} rsf_native_latency_status;
typedef struct rsf_native_latency_frame {
    uint32_t struct_size, version;
    uint64_t source, generation, ready_qpc;
    uint32_t paced, failed, completed;
} rsf_native_latency_frame;
RSF_RUNTIME_API int rsf_native_latency_status_get(rsf_native_latency_status*);
RSF_RUNTIME_API int rsf_native_latency_frame_get(rsf_native_latency_frame*);
/* Runtime graphics paths report their API and execution boundaries. No vendor policy in callers. */
RSF_RUNTIME_API void rsf_native_latency_graphics(uint32_t api);
RSF_RUNTIME_API void rsf_native_latency_render(uint64_t source, uint32_t begin);
RSF_RUNTIME_API void rsf_native_latency_present(uint64_t source, void* chain, uint32_t begin);
/* Diagnostic tool request; registration/message selection remains in the core. */
RSF_RUNTIME_API int rsf_native_latency_ping(void* window);
/* An engine's completed native output can retire earlier CPU sources that never submitted. */
RSF_RUNTIME_API void rsf_native_latency_retire(uint64_t session, uint64_t through);
/* Only after CPU/render/Present producers and their GPU leases have drained. */
RSF_RUNTIME_API void rsf_native_latency_end_session(uint64_t session);
/* Process-wide pacing lease: 1 Direct3D, 2 Vulkan. The first active orchestrator owns it. */
RSF_RUNTIME_API int rsf_native_latency_claim(uint32_t client);
RSF_RUNTIME_API int rsf_native_latency_has_claim(uint32_t client);
RSF_RUNTIME_API void rsf_native_latency_release_claim(uint32_t client);
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
/* Quiescent session shutdown must not clear another active host's facts. */
RSF_RUNTIME_API int rsf_native_cpu_release_session(uint64_t session);
#ifdef __cplusplus
}
#endif
#endif
