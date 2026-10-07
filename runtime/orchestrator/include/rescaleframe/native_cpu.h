/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_CPU_H
#define RSF_NATIVE_CPU_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_native_cpu_frame {
    /* Bits/indexes follow RSF_GAME_CPU_FRAME_BEGIN through FRAME_END. Extra input and pacing
       events use their dedicated fields; every timestamp is a QPC tick at qpc_frequency Hz. */
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
    uint64_t pacing_qpc;
} rsf_native_cpu_frame;
/* Copies CPU ownership/ordering only, with no GPU or presentation authorization. A bounded
   cache may reuse ended records; absent or overwritten IDs refuse lookup. Live records never
   evict. The optional vendor sink runs at the real CPU boundary, outside the cache lock.
   The first FRAME_BEGIN selects one session until release. IDs increase strictly; invalid
   ordering fails the record. FRAME_END can retire partial idle/loading records. */
RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event);
/* Copy an exact session/source record into size-initialized storage; partial/failed records
   remain readable. A successful read does not mean simulation or input is complete. */
RSF_RUNTIME_API int rsf_native_cpu_read(uint64_t session, uint64_t source, rsf_native_cpu_frame* frame);
/* Set/change the sink only while game CPU producers are quiescent. User state must outlive
   callbacks. The sink may sleep/mark the vendor; it must not use the graphics immediate context. */
RSF_RUNTIME_API void rsf_native_cpu_set_sink(rsf_game_cpu_event_fn sink, void* user);
/* After the plugin has stopped CPU producers. */
RSF_RUNTIME_API void rsf_native_cpu_release(void);
#ifdef __cplusplus
}
#endif
#endif
