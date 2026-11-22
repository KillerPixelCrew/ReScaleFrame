/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_cpu_service.h>
#include <rescaleframe/input_dump.h>
#include "../../backends/common/d3d12_readback_layout.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_fg12_texture_desc {
    uint64_t width;
    uint32_t height, format;
} rsf_fg12_texture_desc;
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
