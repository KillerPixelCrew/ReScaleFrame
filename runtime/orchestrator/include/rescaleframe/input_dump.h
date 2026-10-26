/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_INPUT_DUMP_H
#define RSF_INPUT_DUMP_H
#define RSF_INPUT_DUMP_ABI_VERSION 1u
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/backend.h>
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
   A ticket is scoped to one request; retired requests reject stale completions. */
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
/* Certify only source readbacks offered before this call, after the actual source
   queue's submitted work has completed. Used by asynchronous Vulkan readers. */
RSF_FRAME_DUMP_API uint64_t rsf_input_dump_gpu_submission(const rsf_frame_record*);
RSF_FRAME_DUMP_API void rsf_input_dump_gpu_completed(uint64_t submission, uint32_t api, void* device);
RSF_FRAME_DUMP_API int rsf_input_dump_gpu_ready(uint64_t ticket, const char* name, uint32_t api, void* device);
RSF_FRAME_DUMP_API void rsf_input_dump_pass_metadata(const rsf_game_render_pass*);
RSF_FRAME_DUMP_API void rsf_input_dump_result(const rsf_frame_record*, const char* stage, uint32_t backend, int32_t result);
RSF_FRAME_DUMP_API void rsf_input_dump_sr_frame(const rsf_sr_frame*, uint32_t backend, uint32_t input_sdr);
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
