// SPDX-License-Identifier: MIT
// The exported entry point of ReScaleFrame.Game.AC7.dll. Exact-build identity, the GameSDK table,
// the renderer lifecycle and the native renderer are owned by the Rust crate rsf-ac7 (src/lib.rs,
// src/lifecycle.rs, src/native_renderer.rs); this file binds the native renderer's call table,
// which only this DLL links, and asks Rust for the table.
#include "ac7_sites.h"

#include <rescaleframe/game_api.h>

// Rust (lib.rs): validates `requested_abi` and `api` and fills the GameSDK table.
rsf_result rsf_ac7_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api);

__declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api)
{
    // Only this DLL lends the native renderer's MinHook, input fact, scope and factory calls.
    rsf_ac7_native_renderer_bind(&rsf_ac7_native_calls_table);
    return rsf_ac7_publish_api(requested_abi, api);
}
