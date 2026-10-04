/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_FG_H
#define RSF_NATIVE_FG_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/dlss.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_native_fg_options {
    uint32_t struct_size;
    /* frame_limit_us is the minimum interval between rendered frames, before generation.
       Zero is unlimited. */
    uint32_t mode, generated_frames, reflex_mode, frame_limit_us, debug;
} rsf_native_fg_options;
typedef struct rsf_native_fg_status {
    uint32_t struct_size;
    rsf_native_fg_options requested;
    rsf_fg_status vendor;
    uint32_t available, reason;
    int32_t last_result;
    uint64_t source_frame_id, tagged_frames;
} rsf_native_fg_status;
RSF_RUNTIME_API int rsf_native_fg_status_get(rsf_native_fg_status*);
RSF_RUNTIME_API void rsf_native_fg_set_log(rsf_backend_log_fn, void* user);
RSF_RUNTIME_API void rsf_native_fg_options_set(const rsf_native_fg_options*);
/* Zero (default): the game thread's one sleep waits for the previous frame's Present to
   return and still precedes input; RenderSubmitEnd precedes PresentStart. Nonzero: UE's
   one-frame overlap, where that sleep can precede the previous Present. Set before the first frame. */
RSF_RUNTIME_API void rsf_native_fg_reflex_ordering_set(uint32_t threaded);
RSF_RUNTIME_API void rsf_native_fg_cpu(const rsf_game_cpu_event*);
/* Copied identity, scoped around the engine's SR callback. Never uses the latest CPU ID. */
RSF_RUNTIME_API void rsf_native_fg_scene(const rsf_game_render_pass*);
RSF_RUNTIME_API rsf_dlss_result rsf_native_fg_evaluate(void* context11, const rsf_dlss_frame*);
/* Preserve normalized depth/motion independently of the chosen SR provider. */
RSF_RUNTIME_API void rsf_native_fg_capture(void* context11, const rsf_dlss_frame*);
RSF_RUNTIME_API void rsf_native_fg_final(void* context11, const rsf_game_render_pass*);
RSF_RUNTIME_API void rsf_native_fg_submission(const rsf_game_render_pass*, uint32_t begin);
RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass*, uint32_t begin);
RSF_RUNTIME_API void rsf_native_fg_prepare(void* user, void* chain11, void* context11,
    void* list12, void* backbuffer12, rsf_streamline_host*, void* provider, uint32_t sync);
RSF_RUNTIME_API void rsf_native_fg_present(const rsf_observer_present_event*);
RSF_RUNTIME_API void rsf_native_fg_retire(void* user, void* provider);
RSF_RUNTIME_API void rsf_native_fg_window_end(const rsf_game_render_pass*);
#ifdef __cplusplus
}
#endif
#endif
