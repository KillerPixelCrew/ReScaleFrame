// SPDX-License-Identifier: MIT
// Detection, the renderer lifecycle and the Mono helper bootstrap are Rust (lib.rs, lifecycle.rs,
// mono.rs). bridge.cpp, which the Unity package compiles too, keeps the Unity and D3D12 leaves
// over its Rust frame state (bridge_state.rs), so this file lends its entry points to the Rust
// lifecycle owner and keeps the stable plugin export.
#include "bridge.h"
#include "plugin_platform.h"

namespace {
constexpr rsf_unity_plugin_platform platform{
    [](const rsf_game_host_services* host, rsf_unity_native_api* api) noexcept -> uint32_t {
        return host && rsf_unity_bridge_prepare(*host, api) ? 1u : 0u;
    },
    [](uint32_t enabled) noexcept { rsf_unity_bridge_activate(enabled != 0); },
    []() noexcept -> uint32_t { return rsf_unity_bridge_drained() ? 1u : 0u; },
    []() noexcept { rsf_unity_bridge_release(); },
    []() noexcept -> uint32_t { return rsf_unity_bridge_managed_state(); },
};
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_unity_mono_publish_api(requested_abi, api, &platform);
}
