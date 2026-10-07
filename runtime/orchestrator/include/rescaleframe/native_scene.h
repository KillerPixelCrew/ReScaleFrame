/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_SCENE_H
#define RSF_NATIVE_SCENE_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_native_scene_identity {
    /* complete is paired FINAL_SCENE completion; ambiguous rejects duplicate/mismatched views. */
    uint32_t struct_size, complete, ambiguous;
    uint64_t session_id, source_frame_id, viewport_key;
    uint64_t submission_id, family_key, view_key, native_frame, scope_id;
    uint32_t flags;
    int32_t output_rect[4];
    uint64_t surface_key; /* Canonical COM identity only, not a retained/dereferenceable pointer. */
    uint32_t texture_bound;
    uint64_t bound_window_key;
} rsf_native_scene_identity;
/* Graphics execution thread only. Metadata is copied without retaining GPU resources or engine
   pointers. Completion identifies the native family final postprocess pass, not final sampled pixels.
   Returns 1 when accepted and 0 on refusal. begin/end must agree on every stored identity and
   rectangle. The 64-entry thread-local cache never replaces an unfinished unambiguous record. */
RSF_RUNTIME_API int rsf_native_scene_pass(const rsf_game_render_pass* pass, uint32_t begin);
/* Requires a unique completed primary composition from the exact source/viewport/window scope.
   This verifies declared ownership, not which texture Slate sampled or complete FG inputs. */
RSF_RUNTIME_API int rsf_native_scene_texture_binding(const rsf_game_render_pass* pass);
/* Copy the exact completed record into size-initialized caller storage. The output's surface
   key is numeric metadata only; plugin leases must keep any live resource comparisons valid. */
RSF_RUNTIME_API int rsf_native_scene_for_window(const rsf_game_render_pass* window,
    rsf_native_scene_identity* scene);
#define RSF_NATIVE_SCENE_PRESENT_NONE 0u
#define RSF_NATIVE_SCENE_PRESENT_SAMPLED 1u
#define RSF_NATIVE_SCENE_PRESENT_DIRECT 2u
/* Exact completed source/window ownership plus either a sampled scene surface or direct
   rendering into this window's current full-size D3D11 backbuffer. Never dereferences a key. */
RSF_RUNTIME_API uint32_t rsf_native_scene_present_path(const rsf_game_render_pass* window);
/* Clear only this execution thread's records after its users have stopped. */
RSF_RUNTIME_API void rsf_native_scene_reset(void);
#ifdef __cplusplus
}
#endif
#endif
