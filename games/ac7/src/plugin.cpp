// SPDX-License-Identifier: MIT
// Native leaves of ReScaleFrame.Game.AC7.dll. Exact-build identity, the GameSDK table and the
// renderer lifecycle are owned by the Rust crate rsf-ac7 (src/lib.rs, src/lifecycle.rs). This
// file keeps the exported entry point and lends the guarded native leaf snapshot.
#include "ac7_platform.h"
#include "ac7_sites.h"

#include <cstddef>

namespace {
static_assert(sizeof(rsf_ac7_platform) == sizeof(void*));
// Mirrored by src/platform_abi.rs.
static_assert(sizeof(rsf_ac7_native_renderer_options) == 72);
static_assert(offsetof(rsf_ac7_native_renderer_options, session_id) == 8);
static_assert(offsetof(rsf_ac7_native_renderer_options, on_stage) == 16);
static_assert(offsetof(rsf_ac7_native_renderer_options, render_config) == 40);
static_assert(offsetof(rsf_ac7_native_renderer_options, on_state) == 56);
static_assert(offsetof(rsf_ac7_native_renderer_options, diagnostics) == 64);

constexpr rsf_ac7_platform platform{rsf_ac7_get_renderer_leaves};
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_ac7_publish_api(requested_abi, api, &platform);
}
