/* SPDX-License-Identifier: MIT */
#pragma once
/* The Rust lifecycle owner (lifecycle.rs) drives the native D3D12 bridge through this table.
   bridge.cpp stays native because the Unity package compiles it too; the table only lends its
   existing entry points. The rsf_unity_native_api storage handed to bridge_prepare is owned by
   the Rust lifecycle and outlives the managed bootstrap call that copies it. */
#include <rescaleframe/unity_bridge.h>

typedef struct rsf_unity_plugin_platform {
    /* Non-zero when the bridge accepted the host services and filled the managed API table. */
    uint32_t (*bridge_prepare)(const rsf_game_host_services* host, rsf_unity_native_api* api);
    void (*bridge_activate)(uint32_t enabled);
    /* Non-zero when no callback, packet, input hook call or GPU submission is outstanding. */
    uint32_t (*bridge_drained)(void);
    void (*bridge_release)(void);
    /* Managed helper stage: 0 none, 1 loaded, 2 adapter installed, 3 refused, 4 stopped. */
    uint32_t (*bridge_managed_state)(void);
} rsf_unity_plugin_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* Publishes the plugin API. The first accepted table is kept for the lifetime of the DLL. */
rsf_result rsf_unity_mono_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api,
    const rsf_unity_plugin_platform* platform);
#ifdef __cplusplus
}
#endif
