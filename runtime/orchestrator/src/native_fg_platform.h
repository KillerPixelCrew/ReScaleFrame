/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg.h>
#include "native_fg_cpu_platform.h"
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

typedef struct rsf_nfg_texture_info {
    uint32_t width, height, format, samples;
} rsf_nfg_texture_info;

/* Private callback table with every entry present. native_fg.rs decides resources, ordering, leases,
   admission, present sequencing and fallback; each entry performs one platform or vendor
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
    const rsf_nfg_cpu_platform* cpu;
} rsf_nfg_platform;

/* Rust graphics owner. */
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
