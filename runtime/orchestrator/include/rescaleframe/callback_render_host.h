// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_CALLBACK_HOST_ABI_VERSION 1u
/* GPU services for adapters whose callbacks guarantee command-slot retirement.
   Engine startup, symbols and configuration translation stay in the adapter. */
typedef struct rsf_callback_host_setup {
    uint32_t struct_size, abi_version;
    const wchar_t* plugin_path;
    const wchar_t* log_path;
    const wchar_t* preferences_path;
    const rsf_game_probe* probe;
    uint32_t backend, quality, dlss_preset;
    uint32_t sr_backend_choices;
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    const char* streamline_directory_utf8;
    const char* fsr2_directory_utf8;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    int (*set_engine_spatial)(uint32_t spatial, uint32_t quality);
} rsf_callback_host_setup;
RSF_RUNTIME_API uint32_t rsf_callback_host_start(const rsf_callback_host_setup*);
RSF_RUNTIME_API uint32_t rsf_callback_host_stop(void);
#ifdef __cplusplus
}
#endif
