/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>

#ifdef __cplusplus
extern "C" {
#endif

/* DXGI formats, D3D12 states, frame phase and dump views named by native_fg.rs. Pinned against the
   platform headers by static_asserts in native_fg_platform.cpp. */
#define RSF_NFG_FORMAT_RGBA16_FLOAT 10u
#define RSF_NFG_FORMAT_RG32_FLOAT 16u
#define RSF_NFG_FORMAT_RGB10A2_UNORM 24u
#define RSF_NFG_FORMAT_RGBA8_UNORM 28u
#define RSF_NFG_FORMAT_RGBA8_UNORM_SRGB 29u
#define RSF_NFG_FORMAT_RG16_FLOAT 34u
#define RSF_NFG_FORMAT_R32_FLOAT 41u
#define RSF_NFG_FORMAT_BGRA8_UNORM 87u
#define RSF_NFG_FORMAT_BGRA8_UNORM_SRGB 91u
#define RSF_NFG_STATE_COMMON 0u
#define RSF_NFG_STATE_UNORDERED_ACCESS 0x8u
#define RSF_NFG_STATE_NON_PIXEL_SHADER_RESOURCE 0x40u
#define RSF_NFG_STATE_PRESENT 0u
#define RSF_NFG_PHASE_UI_COMPLETE 5u
#define RSF_NFG_DUMP_VIEW_RAW 0u
#define RSF_NFG_DUMP_VIEW_DEPTH 4u
#define RSF_NFG_DUMP_VIEW_DENSE_MOTION 7u
#define RSF_NFG_SR_DLSS 1u
#define RSF_NFG_FG_BACKEND_FSR3 3u
#define RSF_NFG_FG_BACKEND_FSR4 4u
#define RSF_NFG_STAT_LATENCY_AVAILABLE 4u
#define RSF_NFG_PRESENT_TEST 1u
#define RSF_NFG_RESULT_OK 0
#define RSF_NFG_RESULT_NOT_READY (-8)
#define RSF_NFG_RESULT_FEATURE_FAILED (-9)

/* SR failure stages, one diagnostic receipt each, owned by the CPU diagnostics in native_fg.cpp. */
#define RSF_NFG_FAILURE_SR_PREREQUISITE 0u
#define RSF_NFG_FAILURE_SR_TOKEN 1u
#define RSF_NFG_FAILURE_SR_INITIALIZE 2u
#define RSF_NFG_FAILURE_SR_SHARED_EVALUATE 3u

/* CPU ledger counters the graphics owner advances. */
#define RSF_NFG_COUNT_SR_CALL 0u
#define RSF_NFG_COUNT_WINDOW 1u

typedef struct rsf_nfg_texture_info {
    uint32_t width, height, format, samples;
} rsf_nfg_texture_info;

/* Copy of the CPU ledger entry at a source's ring index, which may hold another frame (check id). */
typedef struct rsf_nfg_cpu_owner {
    uint64_t id, session, provider_generation, ready_qpc, source_qpc, source_frequency;
    uint64_t first_submission, first_view, join_ticks, reflex_ticks, submit_begin, submit_end;
    uint32_t submitted, render_complete, paced, prospective, failed;
    uint32_t rhi_begins, scene_begins, scene_ends;
    float source_delta_ms;
} rsf_nfg_cpu_owner;

/* Private callback table with every entry present. native_fg.rs decides resources, ordering, leases,
   admission, present sequencing and fallback; each entry performs one platform, vendor or CPU-ledger
   call. Successful COM outputs transfer one reference; other handles are borrowed unless named. */
typedef struct rsf_nfg_platform {
    /* Presentation bridge and provider. */
    int32_t (*present_has_owner)(void);
    int32_t (*present_is_owner)(void* swapchain);
    void* (*present_host)(void);
    const rsf_generation_provider* (*present_provider)(void);
    void* (*present_session)(void);
    uint32_t (*present_backend)(void);
    uint64_t (*present_generation)(void);
    int32_t (*present_graphics)(void** device12, void** queue12, void** native_device);
    int32_t (*present_retire)(void* provider, void* list, void** fence, uint64_t* value);
    int32_t (*present_abort)(uint64_t id);
    int32_t (*host_token)(void* host, uint64_t id);
    int32_t (*window_match)(void* swapchain, rsf_game_render_pass* out);
    int32_t (*scene_for_window)(const rsf_game_render_pass* window, rsf_native_scene_identity* out);
    uint32_t (*scene_present_path)(const rsf_game_render_pass* window);
    int32_t (*allows_fg)(const rsf_frame_record* record);
    /* D3D11, D3D12 and the shared surface/fence service. */
    void* (*context_device)(void* context);
    void* (*query_context4)(void* context);
    void* (*query_texture)(void* object);
    int32_t (*texture_info)(void* texture, rsf_nfg_texture_info* out);
    void* (*create_event)(void);
    void (*close_event)(void* event);
    void* (*fence_create)(void* device11, void* native_device);
    void* (*fence_d3d11)(void* fence);
    void* (*fence_d3d12)(void* fence);
    void* (*surface_create)(void* device11, void* native_device, uint32_t width, uint32_t height, uint32_t format);
    void (*surface_destroy)(void* surface);
    void* (*surface_d3d11)(void* surface);
    void* (*surface_d3d12)(void* surface);
    uint64_t (*fence_completed)(void* fence12);
    int32_t (*wait_fence)(void* fence12, uint64_t value, void* event);
    void (*flush)(void* context4);
    void (*copy_resource)(void* context4, void* destination, void* source);
    int32_t (*context_signal)(void* context4, void* fence11, uint64_t value);
    int32_t (*context_wait)(void* context4, void* fence11, uint64_t value);
    int32_t (*create_commands)(void* device12, void** allocator, void** list);
    void* (*create_output)(void* device12, uint32_t width, uint32_t height, uint32_t format);
    int32_t (*reset_commands)(void* allocator, void* list);
    int32_t (*close_commands)(void* list);
    void (*execute)(void* queue, void* list);
    void (*transition)(void* list, void* resource, uint32_t before, uint32_t after);
    void (*copy_transitioned)(void* list, void* destination, uint32_t destination_before, uint32_t destination_after,
        void* source, uint32_t source_before, uint32_t source_after);
    int32_t (*queue_signal)(void* queue, void* fence12, uint64_t value);
    int32_t (*queue_wait)(void* queue, void* fence12, uint64_t value);
    /* SR evaluation and frame dumps. */
    int32_t (*dlss_evaluate)(void* context, const rsf_dlss_frame* frame);
    int32_t (*dlss_evaluate_shared)(void* list, const rsf_dlss_frame* frame, uint64_t source);
    void (*dump_sr)(void* context, const rsf_dlss_frame* frame, uint32_t backend);
    void (*dump_fg)(void* context, const rsf_game_render_pass* pass, const rsf_dlss_frame* frame);
    void (*dump_fg_prepare)(const rsf_game_render_pass* pass, const rsf_fg_frame* frame, int32_t result);
    int32_t (*dump_named)(void* context, void* texture, const char* directory, const char* name, uint32_t view,
        float scale, rsf_backend_log_fn log, void* log_user);
    /* CPU pacing and latency marker owner in native_fg.cpp. */
    uint32_t (*cpu_rendered_source)(void);
    void (*cpu_owner_read)(uint64_t source, rsf_nfg_cpu_owner* out);
    void (*cpu_count)(uint64_t source, uint32_t counter);
    void (*cpu_present_observed)(uint64_t presenting);
    void (*cpu_progress)(uint64_t source, uint32_t* live, uint32_t* presented);
    void (*cpu_fail)(uint64_t source);
    void (*cpu_complete)(uint64_t source);
    void (*cpu_latency_present)(uint64_t source, void* chain, uint32_t begin);
    void (*cpu_anti_lag_status)(rsf_fg_status* status);
    void (*cpu_stage)(const char* stage, uint64_t source, uint64_t native_frame, int32_t result,
        const rsf_nfg_cpu_owner* owner, const rsf_game_render_pass* window);
    void (*cpu_trace)(const char* stage, uint64_t source, int32_t result);
    void (*cpu_failure)(uint32_t stage, uint64_t source, uint64_t session, int32_t result, uint32_t detail);
    void (*cpu_report_timing)(const rsf_native_cpu_frame* cpu, const rsf_nfg_cpu_owner* owner, uint32_t screen,
        uint64_t wait_ticks, uint64_t evaluate_ticks);
} rsf_nfg_platform;

/* CPU pacing and latency marker owner (native_fg.cpp), in the order of the table's cpu_* entries. */
uint32_t rsf_nfg_cpu_rendered_source(void);
void rsf_nfg_cpu_owner_read(uint64_t source, rsf_nfg_cpu_owner* out);
void rsf_nfg_cpu_count(uint64_t source, uint32_t counter);
void rsf_nfg_cpu_present_observed(uint64_t presenting);
void rsf_nfg_cpu_progress(uint64_t source, uint32_t* live, uint32_t* presented);
void rsf_nfg_cpu_fail(uint64_t source);
void rsf_nfg_cpu_complete(uint64_t source);
void rsf_nfg_cpu_anti_lag_status(rsf_fg_status* status);
void rsf_nfg_cpu_stage(const char* stage, uint64_t source, uint64_t native_frame, int32_t result,
    const rsf_nfg_cpu_owner* owner, const rsf_game_render_pass* window);
void rsf_nfg_cpu_trace(const char* stage, uint64_t source, int32_t result);
void rsf_nfg_cpu_failure(uint32_t stage, uint64_t source, uint64_t session, int32_t result, uint32_t detail);
void rsf_nfg_cpu_report_timing(const rsf_native_cpu_frame* cpu, const rsf_nfg_cpu_owner* owner, uint32_t screen,
    uint64_t wait_ticks, uint64_t evaluate_ticks);

/* Rust owner (native_fg.rs). Options and the log sink are read by the CPU owner too. */
void rsf_core_native_fg_options(rsf_native_fg_options* out);
void rsf_core_native_fg_logger(rsf_backend_log_fn* sink, void** user);
void rsf_core_native_fg_options_set(const rsf_native_fg_options* options);
void rsf_core_native_fg_set_log(rsf_backend_log_fn sink, void* user);
int rsf_core_native_fg_status_get(rsf_native_fg_status* status, const rsf_nfg_platform* platform);
void rsf_core_native_fg_scene(const rsf_game_render_pass* pass, const rsf_nfg_platform* platform);
void rsf_core_native_fg_final(void* context, const rsf_game_render_pass* pass, const rsf_nfg_platform* platform);
void rsf_core_native_fg_capture(void* context, const rsf_dlss_frame* frame, const rsf_nfg_platform* platform);
int32_t rsf_core_native_fg_evaluate(void* context, const rsf_dlss_frame* frame, const rsf_nfg_platform* platform);
void rsf_core_native_fg_prepare(void* chain, void* list, void* buffer, void* provider, uint32_t sync,
    const rsf_nfg_platform* platform);
void rsf_core_native_fg_retire(void* provider, const rsf_nfg_platform* platform);
void rsf_core_native_fg_present(const rsf_observer_present_event* event, const rsf_nfg_platform* platform);
void rsf_core_native_fg_window_end(const rsf_game_render_pass* pass, const rsf_nfg_platform* platform);
int rsf_core_native_fg_inputs_ready(uint64_t source);
int32_t rsf_core_native_fg_dump_inputs(void* context, uint64_t source, const char* directory, const char* prefix,
    const rsf_nfg_platform* platform);

#ifdef __cplusplus
}
#endif
