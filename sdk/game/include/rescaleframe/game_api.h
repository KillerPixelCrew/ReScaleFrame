// SPDX-License-Identifier: MIT
/* Versioned C ABI for loading a game plugin and controlling its native renderer.
   Both DLLs include these headers; exported functions must not throw or transfer allocator-owned
   objects. A successful detect call identifies an executable, not tested rendering support. */
#ifndef RESCALEFRAME_GAME_API_H
#define RESCALEFRAME_GAME_API_H

#include <stdint.h>
#include <rescaleframe/game_renderer.h>

#define RSF_GAME_ABI_VERSION 13u
#define RSF_GAME_ENTRY_POINT "rsf_get_game_plugin_api"

#ifdef __cplusplus
extern "C" {
#endif

/* Zero indicates success. Negative results describe a refused operation; BUSY means the host
   must retain the plugin and its services while work drains. NATIVE_REFUSED means researched
   engine sites could not be accepted. Callers must inspect status for the current reason. */
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

/* Borrowed executable identity supplied to the plugin's detection callback. */
typedef struct rsf_game_probe {
    uint32_t struct_size;
    /* PE machine type, for example 0x8664 for AMD64. */
    uint16_t pe_machine;
    uint16_t reserved;
    /* Null-terminated UTF-8 executable name and hexadecimal SHA-256, borrowed for the call. */
    const char* executable_name_utf8;
    const char* sha256_hex;
} rsf_game_probe;

/* Immutable plugin identity and user-facing readiness state. Recognition does not imply rendering support. */
typedef struct rsf_game_info {
    const char* id;
    const char* name;
    const char* version;
    uint32_t rendering_ready;
    const char* status;
} rsf_game_info;

/* Read-only executable recognition. Does not prepare or activate renderer hooks. */
typedef rsf_detection (*rsf_detect_game_fn)(const rsf_game_probe* probe);

/* Plugin lifecycle callbacks. Prepare may validate or allocate; start enables hooks; quiesce stops
   new callbacks; stop releases resources after outstanding work drains; status reports current state. */
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

/* Plugin entry point. The caller initializes api->struct_size before calling. The plugin validates
   the requested ABI and fills the versioned function table on success. A short/null table returns
   INVALID_ARGUMENT; an unsupported requested_abi returns ABI_MISMATCH. Resolve the function by
   RSF_GAME_ENTRY_POINT and retain the module until lifecycle stop succeeds. */
typedef rsf_result (*rsf_get_game_plugin_api_fn)(uint32_t requested_abi,
                                               rsf_game_plugin_api* api);

#ifdef __cplusplus
}
#endif
#endif
