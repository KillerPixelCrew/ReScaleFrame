/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/runtime.h>
#include <stdint.h>
#include <rescaleframe/sharpening_settings.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_RENDER_FRONTEND_ABI_VERSION 3u
typedef struct rsf_render_settings {
    uint32_t struct_size, abi_version;
    uint32_t sr_enabled, sr_backend, dlss_preset;
    uint32_t fg_enabled, fg_backend, generated_frames;
    uint32_t latency, frame_limit_us, status_overlay;
    uint32_t sr_quality;
    uint32_t sharpening_mode;
    float sharpening_sharpness;
    uint32_t sharpening_mas;
    float sharpening_motion_sharpness;
    float sharpening_motion_threshold;
    float sharpening_motion_scale_limit;
    uint32_t sharpening_contrast_enabled;
    float sharpening_contrast;
    float sharpening_depth_scale;
    float sharpening_depth_bias;
    uint32_t sharpening_clamp_output;
    uint32_t sharpening_debug;

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
/* The fields rsf_render_settings_check refuses, the reason a request is invalid. */
#define RSF_RENDER_SETTINGS_REFUSED_SR 0x1u         /* sr_enabled, or sr_backend not offered */
#define RSF_RENDER_SETTINGS_REFUSED_QUALITY 0x2u    /* sr_quality the backend does not serve */
#define RSF_RENDER_SETTINGS_REFUSED_PRESET 0x4u     /* dlss_preset */
#define RSF_RENDER_SETTINGS_REFUSED_FG 0x8u         /* fg_enabled (a mode) or fg_backend */
#define RSF_RENDER_SETTINGS_REFUSED_GENERATED 0x10u /* generated_frames outside 1..15 */
#define RSF_RENDER_SETTINGS_REFUSED_LATENCY 0x20u   /* latency above Boost */
#define RSF_RENDER_SETTINGS_REFUSED_OVERLAY 0x40u   /* status_overlay */
#define RSF_RENDER_SETTINGS_REFUSED_SHARPENING 0x80u
/* The Runtime's one check of a settings request, for every host and engine integration:
   provider offered (sr_backend_choices, bit n for backend n; RSF_SR_CHOICES_ALL without a limit
   of its own), quality served by that provider, a valid DLSS preset, a known FG provider and
   mode, generated frames and latency in range, valid sharpening. Returns 0 when accepted,
   otherwise the RSF_RENDER_SETTINGS_REFUSED_* bits; a short or null request refuses every bit. */
RSF_RUNTIME_API uint32_t rsf_render_settings_check(const rsf_render_settings*,
                                                   uint32_t sr_backend_choices);
/* Snapshots contain no borrowed strings or GPU objects. Safe from non-render threads. */
RSF_RUNTIME_API int32_t rsf_render_status_get(rsf_render_status*);
RSF_RUNTIME_API int32_t rsf_render_settings_get(rsf_render_settings*);
RSF_RUNTIME_API int32_t rsf_render_settings_apply(const rsf_render_settings*);
RSF_RUNTIME_API uint32_t rsf_render_overlay_visible(void);
RSF_RUNTIME_API void rsf_render_overlay_toggle(void);
#ifdef __cplusplus
}
#endif
