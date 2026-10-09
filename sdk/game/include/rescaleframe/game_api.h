// SPDX-License-Identifier: MIT
#ifndef RESCALEFRAME_GAME_API_H
#define RESCALEFRAME_GAME_API_H

#include <stdint.h>
#include <rescaleframe/game_renderer.h>

#define RSF_GAME_ABI_VERSION 14u
#define RSF_GAME_ENTRY_POINT "rsf_get_game_plugin_api"

#ifdef __cplusplus
extern "C" {
#endif

typedef int32_t rsf_result;
#define RSF_OK ((rsf_result)0)
#define RSF_ERROR_INVALID_ARGUMENT ((rsf_result)-1)
#define RSF_ERROR_ABI_MISMATCH ((rsf_result)-2)
#define RSF_ERROR_NOT_READY ((rsf_result)-3)
#define RSF_ERROR_BUSY ((rsf_result)-4)
#define RSF_ERROR_NATIVE_REFUSED ((rsf_result)-5)

typedef uint32_t rsf_detection;
#define RSF_GAME_UNKNOWN ((rsf_detection)0)
#define RSF_GAME_RECOGNIZED ((rsf_detection)1)

typedef struct rsf_game_probe {
    uint32_t struct_size;
    uint16_t pe_machine;
    uint16_t reserved;
    const char* executable_name_utf8;
    const char* sha256_hex;
} rsf_game_probe;

/* Recognition describes the executable only. Rendering readiness is separate. */
typedef struct rsf_game_info {
    const char* id;
    const char* name;
    const char* version;
    uint32_t rendering_ready;
    const char* status;
} rsf_game_info;

typedef rsf_detection (*rsf_detect_game_fn)(const rsf_game_probe* probe);
typedef struct rsf_game_hooks_api {
    rsf_result (*prepare)(const rsf_game_prepare_args* args);
    rsf_result (*start)(const rsf_game_start_args* args);
    rsf_result (*quiesce)(const rsf_game_control_args* args);
    rsf_result (*stop)(const rsf_game_control_args* args);
    rsf_result (*status)(rsf_game_renderer_status* status);
} rsf_game_hooks_api;

/* Strings are immutable, plugin-owned, and valid until the DLL is unloaded.
   Probe strings are borrowed for the call and must be null-terminated UTF-8.
   Lifecycle is separate from detection. Prepare may refuse unvalidated native bytes. Stop refuses
   while queued plugin work remains; the host keeps the DLL loaded until stop succeeds. */
typedef struct rsf_game_plugin_api {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_game_info info;
    rsf_detect_game_fn detect;
    rsf_game_hooks_api hooks;
} rsf_game_plugin_api;

typedef rsf_result (*rsf_get_game_plugin_api_fn)(uint32_t requested_abi,
                                               rsf_game_plugin_api* api);

#ifdef __cplusplus
}
#endif
#endif
