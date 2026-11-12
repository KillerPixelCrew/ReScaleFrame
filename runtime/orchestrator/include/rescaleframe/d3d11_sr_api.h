/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_D3D11_SR_API_H
#define RSF_D3D11_SR_API_H

#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/game_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_D3D11_SR_API_VERSION 1u
#define RSF_D3D11_SR_API_ENTRY_POINT "rsf_get_d3d11_sr_api"

/* DLL access to the Runtime's one D3D11 super-resolution pipeline (dlss_pipeline.h) and its
   per-pass evaluation (native_sr.h), for adapters that the carrier loads beside the Runtime. It
   is the pipeline AC7 drives, not a second owner: one process, one device, one Streamline.

   Engine identity, the motion encoding and the depth convention are adapter data in the start
   setup (RSF_DLSS_ENGINE_CUSTOM, view_space_to_meters, project id, motion decode parameters,
   standard_depth). Nothing here selects game-specific behaviour.

   Execution: every entry runs on the thread that owns the D3D11 immediate context, except
   status and request_dump, which are safe from any thread. Nothing is retained from a pass.
   - start: once, with the game's device; refuses ALREADY_RUNNING while started. The setup's
     abi_version is RSF_DLSS_PIPELINE_ABI_VERSION, reported here as pipeline_abi_version.
   - evaluate: rsf_native_sr_evaluate. On success the reconstruction is copied into
     pass->color_output (same format and size as output_texture()), and bindings are restored.
   - reset: the next evaluate starts without history (RSF_GAME_RENDER_RESET is added to it). The
     adapter can also set that flag itself; history also restarts on any size, rectangle,
     session, history key or non-consecutive native_frame change.
   - stop: releases everything, before the caller's device goes. NOT_RUNNING when idle.
   - select_backend/set_quality/set_preset/resize_output: the pipeline functions of the same
     names. Call them at a frame boundary on the context owner; render sizes are returned for
     the engine's next frame. set_preset forwards the user's choice and never picks one.
   A populated table proves ABI availability only, not that any backend runs on this device. */
typedef struct rsf_d3d11_sr_api {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t pipeline_abi_version;
    rsf_dlss_pipeline_result (*start)(void* d3d11_device, const rsf_dlss_pipeline_setup* setup);
    rsf_dlss_pipeline_result (*evaluate)(void* d3d11_context, const rsf_game_render_pass* pass);
    rsf_dlss_pipeline_result (*reset)(void);
    rsf_dlss_pipeline_result (*stop)(void);
    rsf_dlss_pipeline_result (*status)(rsf_dlss_pipeline_status* status);
    rsf_dlss_pipeline_result (*select_backend)(uint32_t backend, uint32_t* render_width,
                                               uint32_t* render_height);
    rsf_dlss_pipeline_result (*set_quality)(uint32_t quality, uint32_t* render_width,
                                            uint32_t* render_height);
    rsf_dlss_pipeline_result (*set_preset)(uint32_t preset);
    rsf_dlss_pipeline_result (*resize_output)(uint32_t output_width, uint32_t output_height,
                                              uint32_t* render_width, uint32_t* render_height);
    /* ID3D11Texture2D* the backend evaluates into, owned by the pipeline; null when stopped. */
    void* (*output_texture)(void);
    rsf_dlss_pipeline_result (*request_dump)(const char* prefix_utf8);
} rsf_d3d11_sr_api;

/* Same contract as rsf_get_d3d11_present_api: initialize struct_size to the writable capacity
   and abi_version to RSF_D3D11_SR_API_VERSION. Null or capacity below sizeof(rsf_d3d11_sr_api):
   INVALID_ARGUMENT, without reading abi_version. Otherwise a version mismatch: ABI_MISMATCH.
   Failure writes nothing. On OK the table is filled and struct_size is the runtime's
   sizeof(table). Querying loads nothing and starts nothing. */
RSF_RUNTIME_API rsf_result rsf_get_d3d11_sr_api(rsf_d3d11_sr_api* api);
typedef rsf_result (*rsf_get_d3d11_sr_api_fn)(rsf_d3d11_sr_api* api);

#ifdef __cplusplus
}
#endif
#endif
