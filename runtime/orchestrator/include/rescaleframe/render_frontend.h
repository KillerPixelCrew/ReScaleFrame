/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/runtime.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_RENDER_FRONTEND_ABI_VERSION 2u
typedef struct rsf_render_settings {
    uint32_t struct_size, abi_version;
    uint32_t sr_enabled, sr_backend, dlss_preset;
    uint32_t fg_enabled, fg_backend, generated_frames;
    uint32_t latency, frame_limit_us, status_overlay;
    uint32_t sr_quality;
} rsf_render_settings;
typedef struct rsf_render_status {
    uint32_t struct_size, abi_version;
    uint32_t sr_enabled, sr_requested_backend, sr_effective_backend;
    uint32_t render_width, render_height, output_width, output_height;
    uint32_t fg_requested_backend, fg_effective_backend, fg_requested, fg_active;
    uint32_t requested_generated, effective_generated, max_generated, foreground;
    uint32_t latency_requested, latency_effective, renderer_reason, fg_reason;
    int32_t sr_result, fg_result, fg_vendor_result;
    uint32_t output_fps_valid, status_overlay;
    uint32_t sr_backend_choices, fg_backend_choices;
    float base_fps, output_fps;
    uint64_t native_frame, source_frame, tagged_frames, application_presents, output_presents;
    char reason[160];
} rsf_render_status;
/* Snapshots contain no borrowed strings or GPU objects. Safe from non-render threads. */
RSF_RUNTIME_API int32_t rsf_render_status_get(rsf_render_status*);
RSF_RUNTIME_API int32_t rsf_render_settings_get(rsf_render_settings*);
RSF_RUNTIME_API int32_t rsf_render_settings_apply(const rsf_render_settings*);
RSF_RUNTIME_API uint32_t rsf_render_overlay_visible(void);
RSF_RUNTIME_API void rsf_render_overlay_toggle(void);
#ifdef __cplusplus
}
#endif
