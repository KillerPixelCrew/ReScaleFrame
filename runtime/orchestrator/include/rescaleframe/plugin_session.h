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
/* Prepare borrows path/probe only during the call. The services table is copied, but its user
   state, callback targets and any referenced host objects must survive through successful stop. */
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
/* Start a prepared session once; repeated starts succeed. A quiesced owner cannot restart.
   A concurrent lifecycle transition or status reader returns BUSY without calling the plugin. */
RSF_RUNTIME_API rsf_result rsf_plugin_session_start(rsf_plugin_session*);
/* Ask the plugin to stop producers and retire work. Successful quiescence is idempotent;
   a refusal leaves the session owned and retryable. No plugin callback runs under the mutex. */
RSF_RUNTIME_API rsf_result rsf_plugin_session_quiesce(rsf_plugin_session*);
/* Fill a caller-initialized renderer status. Multiple readers may overlap; a transition refuses
   new readers. Status pointers/strings retain the game SDK's plugin-owned lifetime. */
RSF_RUNTIME_API rsf_result rsf_plugin_session_status(rsf_plugin_session*, rsf_game_renderer_status*);
/* Null succeeds. A live owner requires successful quiescence and no readers/transition.
   Success deletes the handle and unloads the module; every refusal keeps the handle valid. */
RSF_RUNTIME_API rsf_result rsf_plugin_session_stop(rsf_plugin_session*);
#ifdef __cplusplus
}
#endif
#endif
