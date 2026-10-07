/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/native_fg.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Cold-start native D3D12 presentation. The plugin supplies real CPU events and copies
   normalized depth/motion on its owning queue; generation remains independent of SR.
   Installation runs before graphics creation and installs process-lived interception. */
typedef struct rsf_fg12_setup {
    uint32_t struct_size, backend, max_generated_frames;
    const char* runtime_directory_utf8;
    rsf_native_fg_options options;
    uint32_t depth_inverted, depth_infinite;
    float units_to_meters; /* Positive view-space-to-meter conversion supplied to providers. */
    rsf_backend_log_fn log;
    void* user;
    int (*accept_window)(void*, void* hwnd);
    uint32_t runtime_switching, ui_mode;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    const char* streamline_directory_utf8;
} rsf_fg12_setup;
/* Caller serializes cold startup. Copy settings/install callbacks once; a second install returns
   NEEDS_RESTART. Setup strings are consumed synchronously; callback targets/user remain valid
   for installed hooks. Current
   options accept Off/fixed modes and require nonzero generated_frames. Initial Reflex is forced
   Off; subsequent options requests may enable it when the provider supports it. */
RSF_RUNTIME_API rsf_backend_result rsf_fg12_install(const rsf_fg12_setup*);
/* INI-driven Unity startup before its device/chain exists. Relative SDK paths use the INI's
   directory. Zero means installed/already installed; 1 missing INI, 2 provider, 3 path encoding,
   4 generated count (1..15), 5 interception refusal/exception. No shutdown entry point exists. */
RSF_RUNTIME_API uint32_t __stdcall rsf_unity_fg_start(const wchar_t* configuration);
/* CPU producer callback. Acquire/pacing run before the shared state lock; ordered simulation
   markers and the provider generation bind input timing to the eventual Present. */
RSF_RUNTIME_API void rsf_fg12_cpu(const rsf_game_cpu_event*);
/* Graphics queue owner only. Record copies from NON_PIXEL_SHADER_RESOURCE inputs and restore
   their states. Persistent slot copies keep device depth and dense previous-minus-current
   render-pixel motion; the caller keeps sources alive through actual command submission.
   A second capture of one frame disables interpolation through RESET. */
RSF_RUNTIME_API void rsf_fg12_capture(void* command_list, const rsf_frame_record*,
    void* depth, void* motion);
/* Record matching full-output HUD-less color from RENDER_TARGET and restore that source state.
   Requires depth/motion capture for the same session/source/view. */
RSF_RUNTIME_API void rsf_fg12_hudless(void* command_list, const rsf_game_render_pass*);
/* Confirm that the list containing capture was actually submitted to the owning queue. */
RSF_RUNTIME_API void rsf_fg12_submitted(uint64_t session_id, uint64_t frame_id);
/* Associate the real owned chain's current backbuffer index with copied session/source/view.
   Present consumes that index's identity, so callbacks may occur on different graphics threads. */
RSF_RUNTIME_API void rsf_fg12_window(const rsf_game_render_pass*);
/* Thread-safe copied status; available requires an owned non-Off presentation backend.
   vendor statistic fields are meaningful only when their validity bits are set. */
RSF_RUNTIME_API int rsf_fg12_status(rsf_native_fg_status*);
/* Thread-safe copied intent, applied at Present; invalid mode/count/storage is ignored. */
RSF_RUNTIME_API void rsf_fg12_options(const rsf_native_fg_options*);
#ifdef __cplusplus
}
#endif
