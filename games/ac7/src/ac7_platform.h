// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the AC7 native renderer leaves (plugin.cpp) and their Rust owner
// (lib.rs, lifecycle.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.AC7.dll.
// tools/generate-rust-bindings.py generates src/platform_abi.rs from this header.
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/ac7_native_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

// Exception guards around the native renderer entry points in native_renderer.cpp. A leaf
// returns false only when its native call threw; the Rust lifecycle then answers
// RSF_ERROR_NOT_READY as the former C++ catch did. Results go to the out parameters, which are
// written only when the leaf returns true. The table lives for the process.
typedef struct rsf_ac7_platform {
    // rsf_ac7_native_renderer_prepare. *renderer receives the native output even when *prepared
    // is false: a pinned inactive renderer is returned on a failed partial activation.
    bool (*prepare)(const rsf_ac7_native_renderer_options* options,
        rsf_ac7_native_renderer** renderer, bool* prepared);
    bool (*start)(rsf_ac7_native_renderer* renderer, bool* started);
    bool (*is_active)(rsf_ac7_native_renderer* renderer, bool* active);
    bool (*quiesce)(rsf_ac7_native_renderer* renderer);
    bool (*stop)(rsf_ac7_native_renderer* renderer, bool* stopped);
} rsf_ac7_platform;

// Rust owner: validates `requested_abi` and `api`, retains `platform` and fills the GameSDK table.
rsf_result rsf_ac7_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api,
    const rsf_ac7_platform* platform);

#ifdef __cplusplus
}
#endif
