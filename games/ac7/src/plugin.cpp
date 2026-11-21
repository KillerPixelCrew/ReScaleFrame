// SPDX-License-Identifier: MIT
// Native leaves of ReScaleFrame.Game.AC7.dll. Exact-build identity, the GameSDK table and the
// renderer lifecycle are owned by the Rust crate rsf-ac7 (src/lib.rs, src/lifecycle.rs). This
// file keeps the exported entry point and lends the native renderer entry points behind
// exception guards (ac7_platform.h).
#include "ac7_platform.h"

#include <cstddef>

namespace {
static_assert(sizeof(rsf_ac7_platform) == 5 * sizeof(void*));
// Mirrored by src/platform_abi.rs.
static_assert(sizeof(rsf_ac7_native_renderer_options) == 72);
static_assert(offsetof(rsf_ac7_native_renderer_options, session_id) == 8);
static_assert(offsetof(rsf_ac7_native_renderer_options, on_stage) == 16);
static_assert(offsetof(rsf_ac7_native_renderer_options, render_config) == 40);
static_assert(offsetof(rsf_ac7_native_renderer_options, on_state) == 56);
static_assert(offsetof(rsf_ac7_native_renderer_options, diagnostics) == 64);

// A native call that throws reports false; the Rust lifecycle then answers RSF_ERROR_NOT_READY
// as the previous catch did.
template<class Call> bool guarded(Call&& call) noexcept {
    try { call(); return true; } catch (...) { return false; }
}

constexpr rsf_ac7_platform platform{
    [](const rsf_ac7_native_renderer_options* options, rsf_ac7_native_renderer** renderer,
        bool* prepared) noexcept {
        return guarded([&] {
            rsf_ac7_native_renderer* created = nullptr;
            *prepared = rsf_ac7_native_renderer_prepare(options, &created) != 0;
            *renderer = created;
        });
    },
    [](rsf_ac7_native_renderer* renderer, bool* started) noexcept {
        return guarded([&] { *started = rsf_ac7_native_renderer_start(renderer) != 0; });
    },
    [](rsf_ac7_native_renderer* renderer, bool* active) noexcept {
        return guarded([&] { *active = rsf_ac7_native_renderer_is_active(renderer) != 0; });
    },
    [](rsf_ac7_native_renderer* renderer) noexcept {
        return guarded([&] { rsf_ac7_native_renderer_quiesce(renderer); });
    },
    [](rsf_ac7_native_renderer* renderer, bool* stopped) noexcept {
        return guarded([&] { *stopped = rsf_ac7_native_renderer_stop(renderer) != 0; });
    },
};
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_ac7_publish_api(requested_abi, api, &platform);
}
