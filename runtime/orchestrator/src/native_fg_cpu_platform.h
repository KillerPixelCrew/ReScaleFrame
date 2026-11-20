/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_NFG_CPU_CAPACITY 128u
#define RSF_NFG_CPU_NOT_READY (-8)
#define RSF_NFG_CPU_INVALID (-1)
#define RSF_NFG_CPU_ABI_MISMATCH (-2)
#define RSF_NFG_CPU_SIM_START 0u
#define RSF_NFG_CPU_SIM_END 1u
#define RSF_NFG_CPU_SUBMIT_START 2u
#define RSF_NFG_CPU_SUBMIT_END 3u
#define RSF_NFG_CPU_PRESENT_START 4u
#define RSF_NFG_CPU_PRESENT_END 5u
#define RSF_NFG_CPU_DLSS RSF_FG_BACKEND_DLSS
#define RSF_NFG_CPU_FSR3 RSF_FG_BACKEND_FSR3
#define RSF_NFG_CPU_FSR4 RSF_FG_BACKEND_FSR4

/* Rows supplied to the existing receipt selector through the native bridge. */
typedef struct rsf_nfg_receipt_candidate {
    uint64_t source, session, generation, ready, simulation_begin;
    uint32_t eligible;
} rsf_nfg_receipt_candidate;

/* Private leaves for the D3D CPU owner. Host and device snapshots borrow the
   existing presentation lifetime. Pace and marker calls obtain the provider's
   overlapping read leases inside the presentation bridge. */
typedef struct rsf_nfg_cpu_platform {
    int (*has_owner)(void);
    uint64_t (*generation)(void);
    uint32_t (*backend)(void);
    int32_t (*status)(rsf_d3d11_present_status*);
    void* (*device)(void);
    uint32_t (*adapter_vendor)(void* device);
    void* (*host)(void);
    int32_t (*host_status)(void* host, rsf_streamline_latency_status*);
    int32_t (*host_reflex)(void* host, uint32_t mode, uint32_t interval);
    void* (*host_token)(void* host, uint64_t source);
    int (*host_presented)(void* host, uint64_t source);
    int32_t (*host_abort)(void* host, uint64_t source);
    int32_t (*host_retire)(void* host, uint64_t through);
    int32_t (*host_reset)(void* host);
    int32_t (*acquire)(uint64_t source);
    int32_t (*pace)(uint64_t source, uint32_t pacer, uint64_t generation);
    int32_t (*marker)(uint64_t generation, uint64_t source, uint32_t marker, uint32_t controller);
    int32_t (*input)(uint64_t source, uint32_t kind, uint32_t message);
    int32_t (*receipt)(uint64_t generation, uint64_t source, uint32_t kind, uint32_t message);
    int32_t (*abort)(uint64_t source);
    void* (*create_antilag)(void);
    uint64_t (*receipt_owner)(const rsf_nfg_receipt_candidate*, size_t count,
        uint64_t session, uint64_t generation, uint64_t timestamp);
    uint32_t (*owned_window_thread)(uint64_t window_key);
    int (*post_ping)(void* window, uint32_t message);
    void (*signal_completion)(void);
    int (*wait_completion)(uint32_t timeout_ms, uint32_t pump_sent_messages);
    uint32_t (*input_idle_ms)(void);
    uint64_t (*tick_millis)(void);
    uint64_t (*qpc_frequency)(void);
    uint32_t (*thread_id)(void);
    int (*allows_fg)(const rsf_frame_record*);
    void (*watch_submission)(uint64_t source, uint64_t opened_qpc);
} rsf_nfg_cpu_platform;

extern const rsf_nfg_cpu_platform rsf_native_fg_cpu_platform;

void rsf_core_native_fg_cpu(const rsf_game_cpu_event*, const rsf_nfg_cpu_platform*);
void rsf_core_native_fg_frame(const rsf_game_render_pass*, uint32_t begin, const rsf_nfg_cpu_platform*);
void rsf_core_native_fg_submission(const rsf_game_render_pass*, uint32_t begin, const rsf_nfg_cpu_platform*);
void rsf_core_native_fg_stage(const char*, uint64_t source, uint64_t native_frame, int32_t result,
    const rsf_nfg_cpu_platform*);
void rsf_core_native_fg_reflex_ordering_set(uint32_t);
void rsf_core_native_latency_graphics(uint32_t);
void rsf_core_native_latency_render(uint64_t source, uint32_t begin, const rsf_nfg_cpu_platform*);
void rsf_core_native_latency_present(uint64_t source, void* chain, uint32_t begin, const rsf_nfg_cpu_platform*);
int rsf_core_native_latency_status_get(rsf_native_latency_status*, const rsf_nfg_cpu_platform*);
int rsf_core_native_latency_frame_get(rsf_native_latency_frame*);
int32_t rsf_core_native_latency_window_bind(const rsf_native_latency_window*, const rsf_nfg_cpu_platform*);
int rsf_core_native_input_routes_get(rsf_native_input_routes*);
int rsf_core_native_latency_ping(void* window, const rsf_nfg_cpu_platform*);
void rsf_core_native_latency_retire(uint64_t session, uint64_t through, const rsf_nfg_cpu_platform*);
void rsf_core_native_latency_end_session(uint64_t session, const rsf_nfg_cpu_platform*);

#ifdef __cplusplus
}
#endif
