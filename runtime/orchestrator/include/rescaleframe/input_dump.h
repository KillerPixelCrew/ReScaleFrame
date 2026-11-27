/* SPDX-License-Identifier: MIT */
#ifndef RSF_INPUT_DUMP_H
#define RSF_INPUT_DUMP_H
/* 2: rsf_input_dump_projection. */
#define RSF_INPUT_DUMP_ABI_VERSION 2u
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/backend.h>
#include <rescaleframe/named_frame_dump.h>
#if defined(RSF_FRAME_DUMP_STATIC)
#define RSF_FRAME_DUMP_API
#else
#define RSF_FRAME_DUMP_API RSF_RUNTIME_API
#endif
#ifdef __cplusplus
extern "C" {
#endif
/* All calls run on the graphics execution owner. Nothing retains borrowed textures.
   F10 arms one frame; matching native session/view/frame and window ownership joins its stages. */
RSF_FRAME_DUMP_API void rsf_input_dump_configure(const char* directory, const char* game,
    const char* executable_sha256, rsf_dump_log_fn log, void* user);
RSF_FRAME_DUMP_API void rsf_input_dump_request(void);
/* Stop this owner's request while its GPU readers retire normally. Zero selects any owner. */
RSF_FRAME_DUMP_API void rsf_input_dump_cancel(uint64_t session_id);
/* Core API: hotkey polling or a programmatic request selects the next primary frame.
   Readback reservations keep the bundle open until their actual GPU completions.
   Present completion closes new reservations. A ticket is scoped to one request;
   retired requests reject stale completions. */
RSF_FRAME_DUMP_API void rsf_input_dump_poll(void);
RSF_FRAME_DUMP_API int rsf_input_dump_begin_record(const rsf_frame_record*, uint32_t backend);
RSF_FRAME_DUMP_API int rsf_input_dump_wants(const rsf_frame_record*);
RSF_FRAME_DUMP_API int rsf_input_dump_wants_frame(uint64_t frame);
RSF_FRAME_DUMP_API int rsf_input_dump_current(rsf_frame_record* record);
RSF_FRAME_DUMP_API int rsf_input_dump_requested(void);
RSF_FRAME_DUMP_API uint64_t rsf_input_dump_reserve(const rsf_frame_record*, const char* name,
    const char* description, rsf_dump_view view, float scale);
RSF_FRAME_DUMP_API int rsf_input_dump_pixels(uint64_t ticket, const char* name,
    const rsf_dump_pixels*, uint32_t native_api, uint32_t native_format);
RSF_FRAME_DUMP_API void rsf_input_dump_failed(uint64_t ticket, const char* name, int32_t result);
RSF_FRAME_DUMP_API void rsf_input_dump_complete(const rsf_frame_record*, int32_t result);
/* Compatibility entry points. A frame record cannot certify a command batch.
   Submission and readiness return zero. Completion has no effect. */
RSF_FRAME_DUMP_API uint64_t rsf_input_dump_gpu_submission(const rsf_frame_record*);
RSF_FRAME_DUMP_API void rsf_input_dump_gpu_completed(uint64_t submission, uint32_t api, void* device);
RSF_FRAME_DUMP_API int rsf_input_dump_gpu_ready(uint64_t ticket, const char* name, uint32_t api, void* device);
RSF_FRAME_DUMP_API void rsf_input_dump_pass_metadata(const rsf_game_render_pass*);
/* A successful SR result can supply camera and backend fields missing from the armed record.
   Supply the same session, frame, view, generation and extents before the matched Present. */
RSF_FRAME_DUMP_API void rsf_input_dump_result(const rsf_frame_record*, const char* stage, uint32_t backend, int32_t result);
RSF_FRAME_DUMP_API void rsf_input_dump_sr_frame(const rsf_sr_frame*, uint32_t backend, uint32_t input_sdr);
/* Input dump ABI 2. The projection of a frame whose camera matrices are not known: what a source
   that has near, far and field of view (an upscaler's dispatch parameters, say) can still record.
   A complete camera in the record (rsf_camera_frame) supersedes it; it never makes
   camera_available true. Each value is optional: zero or a non-finite value is "not known".
   Written as projection.json into the bundle of the armed record. A struct_size below this
   layout or another abi_version is refused. */
#define RSF_DUMP_PROJECTION_ABI_VERSION 1u
typedef struct rsf_dump_projection {
    uint32_t struct_size;
    uint32_t abi_version;
    /* As the projection has them; with a reversed depth range the near plane may be the larger. */
    float near_plane;
    float far_plane;
    float vertical_fov_radians;
    /* View width over height. */
    float aspect_ratio;
    /* 1 reversed (near at 1), 0 not, anything else not known. */
    uint32_t depth_inverted;
    /* 1 the far plane is at infinity, 0 not, anything else not known. */
    uint32_t far_infinite;
} rsf_dump_projection;
RSF_FRAME_DUMP_API void rsf_input_dump_projection(const rsf_frame_record*, const rsf_dump_projection*);
/* Immediate-context convenience adapter for D3D11 integrations. Generic frame identity
   is still required; no resource is retained beyond this synchronous snapshot. */
RSF_FRAME_DUMP_API int rsf_input_dump_texture_record(void* context, const rsf_frame_record*,
    const char* name, const char* description, void* texture, rsf_dump_view view, float scale);
RSF_FRAME_DUMP_API int rsf_input_dump_active(void);
RSF_FRAME_DUMP_API void rsf_input_dump_present(void* swapchain, void* context, const rsf_game_render_pass* window);
RSF_FRAME_DUMP_API void rsf_input_dump_presented(const rsf_game_render_pass* window, int32_t result);
RSF_FRAME_DUMP_API void rsf_input_dump_pass(void* context, const rsf_game_render_pass*, uint32_t begin);
RSF_FRAME_DUMP_API void rsf_input_dump_texture(void* context, const char* name, const char* description,
    void* texture, rsf_dump_view view, float scale);
RSF_FRAME_DUMP_API void rsf_input_dump_sr(void* context, const rsf_dlss_frame*, uint32_t backend);
RSF_FRAME_DUMP_API void rsf_input_dump_sr_result(int32_t result);
RSF_FRAME_DUMP_API void rsf_input_dump_fg(void* context, const rsf_game_render_pass*, const rsf_dlss_frame*);
RSF_FRAME_DUMP_API void rsf_input_dump_fg_prepare(const rsf_game_render_pass*, const rsf_fg_frame*, int32_t result);
#ifdef __cplusplus
}
#endif
#endif
