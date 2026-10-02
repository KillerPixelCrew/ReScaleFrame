/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_WINDOW_H
#define RSF_NATIVE_WINDOW_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/d3d11_observer.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Called on the graphics execution thread at queued WINDOW begin/end. Resources remain
   plugin-leased; this owner only keeps copied metadata inside the active execution scope. */
RSF_RUNTIME_API int rsf_native_window_scope(const rsf_game_render_pass* pass, uint32_t begin);
/* Match a real Present callback's COM identity to the current queued window scope. A match
   identifies window/source ownership only, not successful Present or complete FG inputs. */
RSF_RUNTIME_API int rsf_native_window_match(void* swapchain, rsf_game_render_pass* pass);
typedef struct rsf_native_present_record {
    uint32_t struct_size;
    uint32_t matched;
    uint64_t session_id, source_frame_id, scope_id, viewport_key, window_key;
    uint32_t sync_interval, flags;
    int32_t result;
    uint32_t completed;
    /* S_OK was returned for a non-test call, not proof of physical scanout or complete FG input. */
    uint32_t accepted;
    uint64_t begin_qpc, end_qpc, qpc_frequency;
    uint32_t method;
    /* Exact declared composition/source/viewport agreement, not sampled-texture proof. */
    uint32_t scene_matched;
    uint64_t scene_submission_id, scene_view_key, scene_native_frame;
    uint32_t scene_texture_bound;
} rsf_native_present_record;
RSF_RUNTIME_API int rsf_native_window_present(const rsf_observer_present_event* event);
/* Copied numeric data only, no resource borrowed beyond window scope. Graphics owner thread. */
RSF_RUNTIME_API int rsf_native_window_last_present(rsf_native_present_record* record);
RSF_RUNTIME_API void rsf_native_window_reset(void);
#ifdef __cplusplus
}
#endif
#endif
