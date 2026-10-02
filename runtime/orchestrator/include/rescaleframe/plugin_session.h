/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_PLUGIN_SESSION_H
#define RSF_PLUGIN_SESSION_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_plugin_session rsf_plugin_session;
typedef struct rsf_plugin_session_options {
    uint32_t struct_size;
    uint32_t abi_version;
    const wchar_t* plugin_path;
    const rsf_game_probe* probe;
    rsf_game_host_services services;
} rsf_plugin_session_options;
#define RSF_PLUGIN_SESSION_ABI_VERSION 1u
/* Select and prepare the game module before starting a graphics pipeline. The runtime owns the
   module and copied services, and never unloads it while plugin-owned queued work remains.
   Failed prepare may return BUSY with a non-null inactive session: retain that owner and retry
   quiesce/stop. Callers serialize the final successful stop against new operations on its handle. */
RSF_RUNTIME_API rsf_result rsf_plugin_session_prepare(const rsf_plugin_session_options*, rsf_plugin_session** out);
RSF_RUNTIME_API rsf_result rsf_plugin_session_start(rsf_plugin_session*);
RSF_RUNTIME_API rsf_result rsf_plugin_session_quiesce(rsf_plugin_session*);
RSF_RUNTIME_API rsf_result rsf_plugin_session_status(rsf_plugin_session*, rsf_game_renderer_status*);
RSF_RUNTIME_API rsf_result rsf_plugin_session_stop(rsf_plugin_session*);
#ifdef __cplusplus
}
#endif
#endif
