/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/native_fg.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Cold-start native D3D12 presentation. The plugin supplies real CPU events and copies
   normalized depth/motion on its owning queue; generation remains independent of SR. */
typedef struct rsf_fg12_setup {
    uint32_t struct_size, backend, max_generated_frames;
    const char* runtime_directory_utf8;
    rsf_native_fg_options options;
    uint32_t depth_inverted, depth_infinite;
    float units_to_meters;
    rsf_backend_log_fn log;
    void* user;
    int (*accept_window)(void*, void* hwnd);
    uint32_t runtime_switching, ui_mode;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    const char* streamline_directory_utf8;
    /* Streamline engine identity (sl::EngineType value, version, project id), passed to the
       presentation bridge. Without version and project id there is no host and no DLSS-G. */
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
} rsf_fg12_setup;
RSF_RUNTIME_API rsf_backend_result rsf_fg12_install(const rsf_fg12_setup*);
RSF_RUNTIME_API uint32_t __stdcall rsf_unity_fg_start(const wchar_t* configuration);
RSF_RUNTIME_API void rsf_fg12_cpu(const rsf_game_cpu_event*);
RSF_RUNTIME_API void rsf_fg12_capture(void* command_list, const rsf_frame_record*,
    void* depth, void* motion);
RSF_RUNTIME_API void rsf_fg12_hudless(void* command_list, const rsf_game_render_pass*);
/* Confirm that the list containing capture was actually submitted to the owning queue. */
RSF_RUNTIME_API void rsf_fg12_submitted(uint64_t session_id, uint64_t frame_id);
RSF_RUNTIME_API void rsf_fg12_window(const rsf_game_render_pass*);
RSF_RUNTIME_API int rsf_fg12_status(rsf_native_fg_status*);
RSF_RUNTIME_API void rsf_fg12_options(const rsf_native_fg_options*);
#ifdef __cplusplus
}
#endif
