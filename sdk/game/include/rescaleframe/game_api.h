// SPDX-License-Identifier: MIT
#ifndef RESCALEFRAME_GAME_API_H
#define RESCALEFRAME_GAME_API_H

#include <stdint.h>

#define RSF_GAME_ABI_VERSION 1u
#define RSF_GAME_ENTRY_POINT "rsf_get_game_plugin_api"

#ifdef __cplusplus
extern "C" {
#endif

typedef int32_t rsf_result;
#define RSF_OK ((rsf_result)0)
#define RSF_ERROR_INVALID_ARGUMENT ((rsf_result)-1)
#define RSF_ERROR_ABI_MISMATCH ((rsf_result)-2)

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

/* Strings are immutable, plugin-owned, and valid until the DLL is unloaded.
   Probe strings are borrowed for the call and must be null-terminated UTF-8.
   Only metadata/detection is defined in this scaffold; frame callbacks follow
   the renderer experiments before their ABI is committed. */
typedef struct rsf_game_plugin_api {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_game_info info;
    rsf_detect_game_fn detect;
} rsf_game_plugin_api;

typedef rsf_result (*rsf_get_game_plugin_api_fn)(uint32_t requested_abi,
                                               rsf_game_plugin_api* api);

#ifdef __cplusplus
}
#endif
#endif
