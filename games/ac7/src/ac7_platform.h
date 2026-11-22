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

// Acquires a renderer leaf snapshot for one lifecycle operation. A native exception returns
// false and leaves the output untouched. The callbacks in a successful snapshot cannot unwind.
struct rsf_ac7_renderer_leaves;
typedef struct rsf_ac7_platform {
    bool (*renderer_leaves)(struct rsf_ac7_renderer_leaves* out);
} rsf_ac7_platform;

// Rust owner: validates `requested_abi` and `api`, retains `platform` and fills the GameSDK table.
rsf_result rsf_ac7_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api,
    const rsf_ac7_platform* platform);

#ifdef __cplusplus
}
#endif
