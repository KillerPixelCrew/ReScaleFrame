/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_INPUT_DUMP_H
#define RSF_INPUT_DUMP_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/backend.h>
#ifdef __cplusplus
extern "C" {
#endif
/* All calls run on the graphics execution owner. Nothing retains borrowed textures.
   F10 arms one frame; matching native session/view/frame and window ownership joins its stages. */
RSF_RUNTIME_API void rsf_input_dump_configure(const char* directory, const char* game,
    const char* executable_sha256, rsf_dump_log_fn log, void* user);
RSF_RUNTIME_API void rsf_input_dump_request(void);
RSF_RUNTIME_API int rsf_input_dump_active(void);
RSF_RUNTIME_API void rsf_input_dump_present(void* swapchain, void* context);
RSF_RUNTIME_API void rsf_input_dump_presented(void* swapchain, int32_t result);
RSF_RUNTIME_API void rsf_input_dump_pass(void* context, const rsf_game_render_pass*, uint32_t begin);
RSF_RUNTIME_API void rsf_input_dump_texture(void* context, const char* name, const char* description,
    void* texture, rsf_dump_view view, float scale);
RSF_RUNTIME_API void rsf_input_dump_sr(void* context, const rsf_dlss_frame*, uint32_t backend);
RSF_RUNTIME_API void rsf_input_dump_sr_result(int32_t result);
RSF_RUNTIME_API void rsf_input_dump_fg(void* context, const rsf_game_render_pass*, const rsf_dlss_frame*);
RSF_RUNTIME_API void rsf_input_dump_fg_prepare(const rsf_game_render_pass*, const rsf_fg_frame*, int32_t result);
#ifdef __cplusplus
}
#endif
#endif
