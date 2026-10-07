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
    /* available reports a presentation owner, not active generation. Native D3D11 reason:
       0 eligible, 1 window/composition path, 2 inputs, 3 CPU submission, 4 screen, 5 reset, 6 VSync,
       7 remaining identity/CPU prerequisite. Native D3D12 currently uses 0/2 for eligibility.
       vendor supplies actual activity/statistic validity. */
    uint32_t available, reason;
    int32_t last_result;
    uint64_t source_frame_id, tagged_frames;
} rsf_native_fg_status;
/* Thread-safe copied status/settings. Logger user storage must survive all installed callbacks,
   including optional hitch diagnostics. Options are copied; invalid size/modes are ignored. */
RSF_RUNTIME_API int rsf_native_fg_status_get(rsf_native_fg_status*);
RSF_RUNTIME_API void rsf_native_fg_set_log(rsf_backend_log_fn, void* user);
RSF_RUNTIME_API void rsf_native_fg_options_set(const rsf_native_fg_options*);
/* Zero (default): the game thread's one sleep waits for the previous frame's Present to
   return and still precedes input; RenderSubmitEnd precedes PresentStart. Nonzero: UE's
   one-frame overlap, where that sleep can precede the previous Present. Set before the first frame. */
RSF_RUNTIME_API void rsf_native_fg_reflex_ordering_set(uint32_t threaded);
/* CPU/game-thread boundary callback after native_cpu has accepted the event. Reserve at frame
   begin and pace before input. Vendor sleep/join runs without a mutex needed by presentation. */
RSF_RUNTIME_API void rsf_native_fg_cpu(const rsf_game_cpu_event*);
/* Copied identity, scoped around the engine's SR callback. Never uses the latest CPU ID. */
RSF_RUNTIME_API void rsf_native_fg_scene(const rsf_game_render_pass*);
/* Graphics owner only. Use ordinary D3D11 DLSS without a shared host, otherwise transfer inputs
   and evaluate on the shared D3D12 queue using this scope's source token. Dense camera-inclusive
   motion is required for the shared path. Success queues the D3D11 output copy. */
RSF_RUNTIME_API rsf_dlss_result rsf_native_fg_evaluate(void* context11, const rsf_dlss_frame*);
/* Preserve normalized depth/motion independently of the chosen SR provider. Copies go into one
   of six owned slots and never retain the borrowed game textures. Reuse joins GPU/vendor fences. */
RSF_RUNTIME_API void rsf_native_fg_capture(void* context11, const rsf_dlss_frame*);
/* Copy matching completed postprocess color before UI into the slot's HUD-less guide.
   Requires the same session/view/submission as the earlier depth/motion capture. */
RSF_RUNTIME_API void rsf_native_fg_final(void* context11, const rsf_game_render_pass*);
/* Graphics owner begin/end of the real primary scene submission. Opens the render marker once
   and tracks exact source ownership; discarded older CPU frames retire only their SDK tokens. */
RSF_RUNTIME_API void rsf_native_fg_submission(const rsf_game_render_pass*, uint32_t begin);
/* Whole queued RHI frame boundaries. End aborts any unpresented token and releases CPU pacing
   completion. A queued BeginFrame identity alone does not open the vendor rendering interval. */
RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass*, uint32_t begin);
/* Presentation bridge callback before real Present. Validate source/window/final-scene/input
   ownership, CPU submission, history and VSync support before tagging on list12. Temporary
   provider resource states return to COMMON; vendor completion still owns immutable slot data. */
RSF_RUNTIME_API void rsf_native_fg_prepare(void* user, void* chain11, void* context11,
    void* list12, void* backbuffer12, rsf_streamline_host*, void* provider, uint32_t sync);
/* Pair observer Present begin/end for the matched queued window; test calls are ignored.
   Present return closes pacing/latency bookkeeping, without asserting physical scanout. */
RSF_RUNTIME_API void rsf_native_fg_present(const rsf_observer_present_event*);
/* Presentation bridge callback after provider Present/state query. Attach vendor retirement
   fence/value to the tagged slot. A failed query forbids slot reuse until process exit. */
RSF_RUNTIME_API void rsf_native_fg_retire(void* user, void* provider);
/* Close queued window ownership; abort a live token when no matching Present occurred. */
RSF_RUNTIME_API void rsf_native_fg_window_end(const rsf_game_render_pass*);
#ifdef __cplusplus
}
#endif
#endif
