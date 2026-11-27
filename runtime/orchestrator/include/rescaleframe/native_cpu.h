/* SPDX-License-Identifier: MIT */
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
/* Receipt telemetry is independent of per-frame sampled-input attribution. */
typedef struct rsf_native_input_receipts {
    uint32_t struct_size, version;
    uint64_t session_id, accepted, rejected, duplicates;
    uint64_t input_kinds[3], window_messages, ping_candidates;
} rsf_native_input_receipts;
RSF_RUNTIME_API int rsf_native_input_receipts_get(rsf_native_input_receipts*);
RSF_RUNTIME_API int rsf_native_cpu_is_ping_message(uint32_t message);
typedef struct rsf_native_latency_window {
    uint32_t struct_size, version;
    uint64_t session_id, window_key;
} rsf_native_latency_window;
typedef struct rsf_native_input_routes {
    uint32_t struct_size, version;
    uint64_t matched_window, matched_thread, unmatched, routed, failed, last_receipt, last_source;
} rsf_native_input_routes;
RSF_RUNTIME_API int32_t rsf_native_latency_window_bind(const rsf_native_latency_window*);
RSF_RUNTIME_API int rsf_native_input_routes_get(rsf_native_input_routes*);

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
/* Completed CPU events alone cannot authorize a render-submit marker. Report actual submission. */
RSF_RUNTIME_API void rsf_native_latency_render(uint64_t source, uint32_t begin);
RSF_RUNTIME_API void rsf_native_latency_present(uint64_t source, void* chain, uint32_t begin);
/* Diagnostic tool request; registration/message selection remains in the core. */
RSF_RUNTIME_API int rsf_native_latency_ping(void* window);
/* A completed native output can retire earlier CPU sources that never submitted in the active session.
   Zero sources and foreign sessions are ignored. */
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
/* Marker ordering 0..5: 1 accepted, 0 duplicate, -1 missing/out of order.
   Acceptance records ordering only. The provider tracks successful SDK delivery. */
RSF_RUNTIME_API int rsf_native_cpu_marker(uint64_t source, uint32_t marker);
RSF_RUNTIME_API void rsf_native_cpu_paced(uint64_t source, uint64_t began, uint64_t ready);
/* Copies CPU ownership/ordering only, with no GPU or presentation authorization. A bounded
   cache may reuse ended records; absent or overwritten IDs refuse lookup. Live records never
   evict. The optional vendor sink runs at the real CPU boundary, outside the cache lock. */
/* Cold runtime setup: preserve only reported CPU stages; never infer input sampling. */
RSF_RUNTIME_API void rsf_native_cpu_rendered_source_set(uint32_t enabled);
RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event);
/* Runtime hosts deliver accepted and normalized facts to their backend endpoint. No adapter supplies this sink.
   Refused prospective consumption retains real simulation facts without synthesizing
   input or forwarding their latency stages; frame open/close still reaches the sink. */
RSF_RUNTIME_API int rsf_native_cpu_dispatch(const rsf_game_cpu_event* event, rsf_game_cpu_event_fn sink, void* user);
RSF_RUNTIME_API int rsf_native_cpu_read(uint64_t session, uint64_t source, rsf_native_cpu_frame* frame);
/* Set/change the sink only while game CPU producers are quiescent. User state must outlive
   callbacks. The sink may sleep/mark the vendor; it must not use the graphics immediate context. */
/* Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
   work (see docs/representation-plan.md). Do not delete as dead code. */
RSF_RUNTIME_API void rsf_native_cpu_set_sink(rsf_game_cpu_event_fn sink, void* user);
/* Prospective source reservations (CPU event version 2). Validated facts reach the
   existing runtime sink synchronously outside rendered-only mode, after releasing
   the ledger lock. The runtime may pace RESERVE; the ledger never calls a vendor. */
#define RSF_NATIVE_CPU_RESERVATION_OPEN 1u     /* poll in flight */
#define RSF_NATIVE_CPU_RESERVATION_SAMPLED 2u  /* poll returned; consuming source unknown */
#define RSF_NATIVE_CPU_RESERVATION_ADOPTED 3u  /* FRAME_BEGIN used the identity; not yet bound */
#define RSF_NATIVE_CPU_RESERVATION_BOUND 4u    /* consumption bound inside the open source */
#define RSF_NATIVE_CPU_RESERVATION_CLOSED 5u   /* bound source ended */
#define RSF_NATIVE_CPU_RESERVATION_UNBOUND 6u  /* adopted source ended without binding */
#define RSF_NATIVE_CPU_RESERVATION_ABORTED 7u
/* Predecessor actual-Present outcome, as known when the reservation is read. */
#define RSF_NATIVE_CPU_DEPENDENCY_NONE (-1)     /* no predecessor reported */
#define RSF_NATIVE_CPU_DEPENDENCY_PENDING 0
#define RSF_NATIVE_CPU_DEPENDENCY_PRESENTED 1   /* the actual call returned success */
#define RSF_NATIVE_CPU_DEPENDENCY_SUPERSEDED 2  /* a later source presented; none reported for this one */
#define RSF_NATIVE_CPU_DEPENDENCY_CANCELLED 3   /* the engine will not present it */
#define RSF_NATIVE_CPU_DEPENDENCY_FAILED 4      /* the actual call returned failure */
#define RSF_NATIVE_CPU_DEPENDENCY_UNVERIFIED 5  /* returned without a result; not success */
typedef struct rsf_native_cpu_reservation {
    uint32_t struct_size, state;
    uint64_t session_id, source_frame_id, predecessor_source_id, native_generation, qpc_frequency;
    uint64_t reserve_qpc, sampled_qpc, frame_begin_qpc, bind_qpc;
    uint32_t input_kind, flags, abort_reason;
    int32_t predecessor_present; /* RSF_NATIVE_CPU_DEPENDENCY_* at read time */
    uint32_t padding0;
    /* Actual input sample: native poll entry after RESERVE returned. Zero when unreported.
       reserve_qpc precedes any runtime work inside RESERVE; sampled_qpc is the poll's return. */
    uint64_t poll_begin_qpc;
} rsf_native_cpu_reservation;
typedef struct rsf_native_cpu_reservation_counters {
    uint32_t struct_size;
    uint64_t reserved, sampled, adopted, bound, closed, unbound, aborted, rejected;
} rsf_native_cpu_reservation_counters;
RSF_RUNTIME_API int rsf_native_cpu_reservation_read(uint64_t session, uint64_t source, rsf_native_cpu_reservation* out);
RSF_RUNTIME_API int rsf_native_cpu_reservation_counters_get(rsf_native_cpu_reservation_counters* out);
/* Actual application Present outcome for a copied session/source identity, reported after the
   real call returned (RETURNED with its native result, RESULT_UNKNOWN when it is unavailable) or
   when the engine will not present that source (ABORTED). Independent of FRAME_END and binding. */
#define RSF_NATIVE_CPU_PRESENT_RETURNED 1u
#define RSF_NATIVE_CPU_PRESENT_RESULT_UNKNOWN 2u
#define RSF_NATIVE_CPU_PRESENT_ABORTED 3u
RSF_RUNTIME_API int rsf_native_cpu_present_result(uint64_t session, uint64_t source, uint32_t outcome, int32_t native_result);
/* After the plugin has stopped CPU producers. */
RSF_RUNTIME_API void rsf_native_cpu_release(void);
/* Quiescent session shutdown must not clear another active host's facts. */
RSF_RUNTIME_API int rsf_native_cpu_release_session(uint64_t session);
#ifdef __cplusplus
}
#endif
#endif
