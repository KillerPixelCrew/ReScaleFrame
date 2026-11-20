/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/motion_resolve.h>
#include <rescaleframe/sharpening_settings.h>
#include "d3d11_output_platform.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Private D3D11 SR pipeline bridge. dlss_pipeline.rs owns the provider session, backend and
   quality switching, frame sequencing, reset and fallback, resource leases, output publication,
   the layer feature and teardown. Every entry below is one existing SDK, graphics or diagnostic
   call. Entries never throw. Textures returned by create_texture and create_zeroed carry one
   reference. The Rust mirror in dlss_pipeline_abi.rs is hand-written and checked by the size
   assertions in dlss_pipeline.cpp. */
typedef void (*rsf_dlssp_log_fn)(void* user, const char* message);

/* rsf_dlss_plan's answer. */
typedef struct rsf_dlssp_range {
    uint32_t width, height, min_width, min_height, max_width, max_height;
} rsf_dlssp_range;

/* rsf_dlss_support's answer. */
typedef struct rsf_dlssp_support {
    uint32_t supported, driver_out_of_date, os_out_of_date, no_supported_adapter;
    uint32_t required_major, required_minor, detected_major, detected_minor;
} rsf_dlssp_support;

/* The frame facts the D3D11 sharpening pass reads besides its options. */
typedef struct rsf_dlssp_sharpen_frame {
    uint32_t depth_inverted;
    float motion_x, motion_y, near_plane, far_plane;
} rsf_dlssp_sharpen_frame;

typedef struct rsf_dlssp_platform {
    /* Streamline and the DLSS feature. Results are rsf_dlss_result. */
    void* (*present_host)(void);
    int32_t (*share_host)(void* host, rsf_dlssp_log_fn log, void* user);
    int32_t (*load)(const rsf_dlss_pipeline_setup* setup, const char* interposer, const char* plugins,
        rsf_dlssp_log_fn log);
    int32_t (*set_device)(void* device);
    int32_t (*query_support)(rsf_dlssp_support* support);
    int32_t (*plan)(uint32_t output_width, uint32_t output_height, uint32_t quality, rsf_dlssp_range* range);
    int32_t (*release_resources)(void);
    int32_t (*release_viewport)(uint32_t viewport);
    int32_t (*shutdown)(void);
    int32_t (*evaluate_layer)(void* context, const rsf_dlss_frame* frame);
    int32_t (*preset_valid)(uint32_t preset);
    /* Native FG owner and the native SR owner's cached resources. */
    int32_t (*fg_evaluate)(void* context, const rsf_dlss_frame* frame);
    void (*fg_capture)(void* context, const rsf_dlss_frame* frame);
    void (*native_sr_release)(void);
    /* D3D11 textures: the d3d11_output leaves plus two initialisers. */
    void (*texture_desc)(void* texture, rsf_d3d11_texture_desc* desc);
    void* (*create_texture)(void* device, const rsf_d3d11_texture_desc* desc);
    void (*clear_black)(void* device, void* texture);
    void* (*create_zeroed)(void* device, const rsf_d3d11_texture_desc* desc, uint32_t row_pitch);
    /* Motion decode and dense resolve passes. decode_create returns rsf_motion_decode_result. */
    int32_t (*decode_create)(void* device, uint32_t width, uint32_t height, uint32_t format,
        rsf_dlssp_log_fn log, void** decode);
    int32_t (*decode_run)(void* decode, void* context, void* motion, const rsf_motion_decode_params* params);
    void* (*decode_texture)(void* decode);
    void (*decode_destroy)(void* decode);
    void* (*resolve_create)(void* device, uint32_t width, uint32_t height);
    int32_t (*resolve_run)(void* resolve, void* context, void* motion, void* depth,
        const rsf_motion_resolve_params* params);
    void* (*resolve_motion)(void* resolve);
    void* (*resolve_depth)(void* resolve);
    void (*resolve_destroy)(void* resolve);
    /* Colour correction and colour transport passes. */
    void* (*fidelity_create)(void* device);
    int32_t (*fidelity_run)(void* pass, void* context, void* scene, void* output, void* depth,
        uint32_t depth_inverted, const float jitter[2]);
    void (*fidelity_destroy)(void* pass);
    void* (*transport_create)(void* device);
    int32_t (*transport_encode)(void* transport, void* context, void* source, void* exposure,
        uint32_t width, uint32_t height, void** encoded);
    int32_t (*transport_decode)(void* transport, void* context, void* target, void* exposure,
        uint32_t width, uint32_t height, void* scene, uint32_t scene_width, uint32_t scene_height,
        const float jitter[2]);
    void (*transport_destroy)(void* transport);
    /* Sharpening settings and the post-SR pass. sharpen returns one when the options are inactive
       or the pass ran, zero when an active pass was unavailable or refused; it creates the pass in
       *pass on first active use. */
    int32_t (*sharpening_valid)(const rsf_sharpening_options* options);
    void (*sharpening_defaults)(rsf_sharpening_options* options);
    float (*vendor_sharpness)(const rsf_sharpening_options* options, uint32_t backend);
    int32_t (*sharpen)(void** pass, void* context, void* output, void* motion, void* depth, void* exposure,
        const rsf_sharpening_options* options, const rsf_dlssp_sharpen_frame* frame);
    void (*sharpen_destroy)(void* pass);
    /* The legacy FSR/XeSS adapter (sr_legacy.rs) behind its existing native constructor. */
    int32_t (*alternate_create)(void* device, uint32_t width, uint32_t height, const char* const* directories,
        float units_to_meters, uint32_t standard_depth, uint32_t auto_exposure, rsf_dlssp_log_fn log,
        void* user, void** adapter);
    /* Frame capture and the request dump. Dump results are rsf_dump_texture_result. */
    void (*input_dump)(void* context, const char* name, const char* description, void* texture,
        uint32_t view, float scale);
    int32_t (*dump_bytes)(void* device, void* context, void* texture, const char* prefix, uint32_t view,
        float scale, rsf_dlssp_log_fn log);
    int32_t (*dump_image)(void* device, void* context, void* texture, const char* prefix, uint32_t view,
        float scale, rsf_dlssp_log_fn log);
} rsf_dlssp_platform;

#ifdef __cplusplus
}
#endif
