/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/input_dump.h>

#ifdef __cplusplus
extern "C" {
#endif

/* COM outputs transfer one reference, except resource arguments borrowed for a call. */
uint32_t rsf_fg12_platform_buffer_index(void* chain);
int rsf_fg12_platform_install(const rsf_d3d11_present_setup*, int explicit_creation);
uint32_t rsf_fg12_platform_allows_fg(const rsf_frame_record*);
uint32_t rsf_fg12_platform_depth_domains(const rsf_game_render_pass*);

typedef struct rsf_fg12_texture_desc {
    uint64_t width;
    uint32_t height, format;
} rsf_fg12_texture_desc;
void rsf_fg12_platform_texture_desc(void* resource, rsf_fg12_texture_desc*);
void* rsf_fg12_platform_texture(void* list, void* resource, void* reuse);
void rsf_fg12_platform_copy(void* list, void* source, uint32_t source_state, void* target, uint32_t target_state);
void rsf_fg12_platform_transition(void* list, void* resource, uint32_t before, uint32_t after);
void* rsf_fg12_platform_fence(void* queue);
int rsf_fg12_platform_signal(void* queue, void* fence, uint64_t value);

typedef struct rsf_fg12_readback_layout {
    uint64_t bytes, offset;
    uint32_t width, height, format, row_pitch;
} rsf_fg12_readback_layout;
void* rsf_fg12_platform_readback(void* source, rsf_fg12_readback_layout*);
void rsf_fg12_platform_readback_copy(void* list, void* source, uint32_t state, void* buffer, const rsf_fg12_readback_layout*);
const uint8_t* rsf_fg12_platform_map(void* buffer, uint64_t bytes);
void rsf_fg12_platform_unmap(void* buffer);

/* Shared frame-capture backend. Rust supplies the actual frame and queue boundaries. */
void rsf_fg12_platform_dump_record(void* list, void* source, uint32_t state, const rsf_frame_record*, const char* role, uint32_t slot);
void rsf_fg12_platform_dump_flush(void);
void rsf_fg12_platform_dump_submitted(void* queue, const rsf_frame_record*);
void rsf_fg12_platform_dump_prepare(const rsf_fg_frame*, rsf_backend_result);

/* Registers the table below once. Owners that hand inputs to FG12 inside the Rust core call it
   first, exactly as the public FG12 entries do. */
void rsf_fg12_platform_bind(void);

/* Registered by the full presentation host. CPU-only binaries link the same Rust
   archive without pulling the DXGI facade or vendor backend objects. */
typedef struct rsf_fg12_calls {
    uint32_t struct_size;
    uint32_t (*buffer_index)(void*);
    int (*install)(const rsf_d3d11_present_setup*, int);
    uint32_t (*allows_fg)(const rsf_frame_record*);
    uint32_t (*depth_domains)(const rsf_game_render_pass*);
    void (*texture_desc)(void*, rsf_fg12_texture_desc*);
    void* (*texture)(void*, void*, void*);
    void (*copy)(void*, void*, uint32_t, void*, uint32_t);
    void (*transition)(void*, void*, uint32_t, uint32_t);
    void* (*fence)(void*);
    int (*signal)(void*, void*, uint64_t);
    void* (*readback)(void*, rsf_fg12_readback_layout*);
    void (*readback_copy)(void*, void*, uint32_t, void*, const rsf_fg12_readback_layout*);
    const uint8_t* (*map)(void*, uint64_t);
    void (*unmap)(void*);
    void (*dump_record)(void*, void*, uint32_t, const rsf_frame_record*, const char*, uint32_t);
    void (*dump_flush)(void);
    void (*dump_submitted)(void*, const rsf_frame_record*);
    void (*dump_prepare)(const rsf_fg_frame*, rsf_backend_result);
    void* (*present_queue)(void);
    void* (*source_queue)(void);
    int32_t (*present_retire)(void*, void*, void**, uint64_t*);
    int (*is_owner)(void*);
    int (*has_owner)(void);
    const rsf_generation_provider* (*present_provider)(void);
    uint32_t (*present_backend)(void);
    uint64_t (*present_generation)(void);
    rsf_backend_result (*host_abort)(rsf_streamline_host*, uint64_t);
    void (*fg_set_log)(rsf_backend_log_fn, void*);
    void (*fg_options_set)(const rsf_native_fg_options*);
    void (*fg_cpu)(const rsf_game_cpu_event*);
    int (*latency_status_get)(rsf_native_latency_status*);
    int (*latency_frame_get)(rsf_native_latency_frame*);
    void (*latency_graphics)(uint32_t);
    void (*latency_render)(uint64_t, uint32_t);
    void (*latency_present)(uint64_t, void*, uint32_t);
    void (*latency_retire)(uint64_t, uint64_t);
    void (*latency_end_session)(uint64_t);
    int (*cpu_dispatch)(const rsf_game_cpu_event*, rsf_game_cpu_event_fn, void*);
    void (*dump_poll)(void);
    int (*dump_begin_record)(const rsf_frame_record*, uint32_t);
    int (*dump_current)(rsf_frame_record*);
    int (*dump_requested)(void);
    void (*dump_complete)(const rsf_frame_record*, int32_t);
    void (*dump_result)(const rsf_frame_record*, const char*, uint32_t, int32_t);
} rsf_fg12_calls;

#ifdef __cplusplus
}
#endif
